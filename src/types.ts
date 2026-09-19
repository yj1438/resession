// IR 的 TS 镜像 —— 与 crates/session-core/src/ir.rs 保持同步（docs/data-formats.md 第 2 节）
//
// ⚠ 契约：本文件所有类型必须与 Rust 侧序列化结果逐字段对齐（camelCase）。
// Rust 侧由 crates/session-core/tests/dto_contracts.rs 与 src-tauri 各模块
// 的契约测试把关。新增/修改 DTO 三步：rename_all camelCase → 补契约断言 → 改这里。

export interface SessionMeta {
  provider: string;
  id: string;
  cwd: string | null;
  projectDir: string;
  title: string | null;
  createdAt: string | null;
  modifiedAt: string | null;
  messageCount: number;
  sourceFile: string;
}

export type Role = "user" | "assistant" | "system";

export type Block =
  | { kind: "text"; text: string }
  | { kind: "toolUse"; name: string; brief: string }
  | { kind: "toolResult"; brief: string };

export interface Event {
  role: Role;
  timestamp: string | null;
  blocks: Block[];
  sidechain: boolean;
}

export interface PtyStatus {
  id: string;
  lastOutputMs: number;
  cwd: string;
}

export interface SearchHit {
  session: SessionMeta;
  eventIndex: number;
  role: Role;
  sidechain: boolean;
  snippet: string;
}

export interface AppSettings {
  aliases: Record<string, string>;
  claudePath: string | null;
  busyMs: number;
}
