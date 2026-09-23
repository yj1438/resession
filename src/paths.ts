/** 去掉末尾路径分隔符，同时兼容 Windows 与 Unix 路径。 */
function trimTrailingSeparators(path: string): string {
  return path.replace(/[\\/]+$/, "");
}

/** 用于 UI 展示的最后一级目录名；根路径保留原值。 */
export function pathBaseName(path: string): string {
  const trimmed = trimTrailingSeparators(path);
  const parts = trimmed.split(/[\\/]/).filter(Boolean);
  return parts.at(-1) ?? path;
}

/** macOS 文件系统（APFS/HFS+）默认大小写不敏感，比较语义应与 Windows 看齐。 */
function isCaseInsensitiveFs(): boolean {
  if (typeof navigator === "undefined") return false;
  // navigator.platform 已废弃但 WKWebView 仍提供；userAgent 兜底
  return /mac/i.test(navigator.platform ?? "") || /\bMac\b/.test(navigator.userAgent);
}

/**
 * 用于路径比较/分组的稳定形式。
 * Windows 与 macOS 的文件系统大小写不敏感，比较时归一；
 * Linux 保留大小写语义。展示路径一律用原始值，不经过本函数。
 */
export function normalizePath(path: string): string {
  const normalized = trimTrailingSeparators(path).replace(/\\/g, "/");
  const isWinDrive = /^[a-z]:\//i.test(normalized);
  return isWinDrive || isCaseInsensitiveFs() ? normalized.toLowerCase() : normalized;
}

export function pathsEqual(left: string, right: string): boolean {
  return normalizePath(left) === normalizePath(right);
}
