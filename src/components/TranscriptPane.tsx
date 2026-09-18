import type { SessionMeta } from "../types";

// M0：仅展示会话元信息，证明选中链路通；富文本转录渲染在 M1/M2
// 接入 invoke("load_transcript") 后替换整个内容区
export default function TranscriptPane({ session }: { session: SessionMeta }) {
  return (
    <div className="transcript">
      <h3>{session.title ?? "(无标题)"}</h3>
      <dl>
        <dt>会话 ID</dt>
        <dd className="mono">{session.id}</dd>
        <dt>项目目录</dt>
        <dd className="mono">{session.cwd ?? session.projectDir}</dd>
        <dt>消息数</dt>
        <dd>{session.messageCount}</dd>
        <dt>转录渲染</dt>
        <dd className="hint">M1 接入 invoke(load_transcript) 后显示 IR 转录</dd>
      </dl>
    </div>
  );
}
