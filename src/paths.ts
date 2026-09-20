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

/**
 * 用于路径比较/分组的稳定形式。
 * Windows 路径统一分隔符并忽略大小写；Unix 路径保留大小写语义。
 */
export function normalizePath(path: string): string {
  const normalized = trimTrailingSeparators(path).replace(/\\/g, "/");
  return /^[a-z]:\//i.test(normalized) ? normalized.toLowerCase() : normalized;
}

export function pathsEqual(left: string, right: string): boolean {
  return normalizePath(left) === normalizePath(right);
}
