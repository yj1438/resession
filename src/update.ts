import { getVersion } from "@tauri-apps/api/app";

export interface UpdateInfo {
  current: string;
  latest: string;
  hasUpdate: boolean;
  url: string;
}

// 简化语义版本比较（v 前缀与 -pre 后缀容忍；只比三段数字）
function verTriple(v: string): [number, number, number] {
  const parts = v
    .replace(/^v/, "")
    .split("-")[0]
    .split(".")
    .map((n) => parseInt(n, 10) || 0);
  return [parts[0] ?? 0, parts[1] ?? 0, parts[2] ?? 0];
}

function isNewer(latest: string, current: string): boolean {
  const [a, b, c] = verTriple(latest);
  const [x, y, z] = verTriple(current);
  return a !== x ? a > x : b !== y ? b > y : c !== z ? c > z : false;
}

// 查询 GitHub Releases 最新版本（公开接口，无鉴权；CORS 由 GitHub 开放）
export async function checkUpdate(): Promise<UpdateInfo> {
  const current = await getVersion();
  const res = await fetch(
    "https://api.github.com/repos/yj1438/resession/releases/latest",
    { headers: { Accept: "application/vnd.github+json" } },
  );
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const rel = (await res.json()) as { tag_name?: string; html_url?: string };
  const latest = String(rel.tag_name ?? "");
  const url = String(
    rel.html_url ?? "https://github.com/yj1438/resession/releases",
  );
  return { current, latest, hasUpdate: isNewer(latest, current), url };
}
