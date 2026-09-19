import type { ReactNode } from "react";

// 大小写不敏感地把 query 出现处包上 <mark>
export default function Highlight({
  text,
  query,
}: {
  text: string;
  query?: string;
}) {
  const q = query?.trim();
  if (!q) return <>{text}</>;
  const lower = text.toLowerCase();
  const ql = q.toLowerCase();
  const parts: ReactNode[] = [];
  let i = 0;
  let k = lower.indexOf(ql);
  let key = 0;
  while (k !== -1) {
    if (k > i) parts.push(text.slice(i, k));
    parts.push(<mark key={key++}>{text.slice(k, k + q.length)}</mark>);
    i = k + q.length;
    k = lower.indexOf(ql, i);
  }
  parts.push(text.slice(i));
  return <>{parts}</>;
}
