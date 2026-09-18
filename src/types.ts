// IR 的 TS 镜像 —— 与 crates/session-core/src/ir.rs 保持同步（docs/data-formats.md 第 2 节）

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
}
