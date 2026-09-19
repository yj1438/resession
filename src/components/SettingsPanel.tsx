import { useState } from "react";
import { invoke, isTauri } from "../api";
import type { AppSettings } from "../types";

// 设置面板：claude 路径覆盖 / 忙闲阈值 / 别名管理。
// 保存走 save_settings（后端同步更新二进制 override），别名删除即时生效。
export default function SettingsPanel({
  settings,
  onClose,
  onSaved,
}: {
  settings: AppSettings;
  onClose: () => void;
  onSaved: (s: AppSettings) => void;
}) {
  const [claudePath, setClaudePath] = useState(settings.claudePath ?? "");
  const [busyMs, setBusyMs] = useState(String(settings.busyMs));
  const [aliases, setAliases] = useState(settings.aliases);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const save = async () => {
    if (!isTauri) return;
    try {
      const s = await invoke<AppSettings>("save_settings", {
        claudePath: claudePath.trim() || null,
        busyMs: Number(busyMs) || 4000,
      });
      onSaved(s);
      setSaved(true);
      setError(null);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      setError(String(e));
    }
  };

  const removeAlias = async (key: string) => {
    if (!isTauri) return;
    try {
      await invoke("remove_alias", { key });
      setAliases((prev) => {
        const next = { ...prev };
        delete next[key];
        return next;
      });
    } catch (e) {
      setError(String(e));
    }
  };

  const aliasEntries = Object.entries(aliases);

  return (
    <div className="np-backdrop" onClick={onClose}>
      <div className="np" onClick={(e) => e.stopPropagation()}>
        <div className="np-head">设置</div>
        <div className="settings-body">
          <label className="set-label">
            claude 可执行路径（留空 = 自动探测 PATH 与常见安装位置）
          </label>
          <input
            className="np-input full"
            value={claudePath}
            placeholder="例如 C:\Users\you\AppData\Roaming\npm\claude.cmd"
            onChange={(e) => setClaudePath(e.target.value)}
          />

          <label className="set-label">
            忙闲判定阈值（毫秒，终端输出静默超过该时长视为空闲，1000-30000）
          </label>
          <input
            className="np-input"
            type="number"
            min={1000}
            max={30000}
            step={500}
            value={busyMs}
            onChange={(e) => setBusyMs(e.target.value)}
          />

          <label className="set-label">
            会话别名（{aliasEntries.length} 个；重命名不影响原生会话数据）
          </label>
          <ul className="alias-list">
            {aliasEntries.map(([key, name]) => (
              <li key={key}>
                <span className="alias-name">{name}</span>
                <span className="mono alias-key">{key}</span>
                <button className="term-close" onClick={() => void removeAlias(key)}>
                  删除
                </button>
              </li>
            ))}
            {aliasEntries.length === 0 && (
              <li className="hint">暂无别名（在会话列表双击标题即可创建）</li>
            )}
          </ul>
        </div>
        <div className="np-actions">
          <button className="np-pick" onClick={() => void save()}>
            保存
          </button>
          <span className="hint">{saved ? "已保存 ✓" : error ?? ""}</span>
          <button className="np-pick close-right" onClick={onClose}>
            关闭
          </button>
        </div>
      </div>
    </div>
  );
}
