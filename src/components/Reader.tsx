import { useStore } from "../store";
import { Tag } from "./Tag";
import { MessageView } from "./MessageView";
import { IconCopy, IconFolder, IconTerminal } from "./icons";

export function Reader() {
  const { activeSession, transcript, loadingTranscript, copyCommand, openTerminal } = useStore();

  if (!activeSession) {
    return (
      <section className="reader">
        <div className="empty">
          <div>
            <div className="ico"><IconFolder size={24} /></div>
            选择一个会话查看完整对话
          </div>
        </div>
      </section>
    );
  }

  const s = activeSession;
  const time = s.updated_at.match(/(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2})/);
  const timeStr = time ? `${time[2]}-${time[3]} ${time[4]}:${time[5]}` : s.updated_at.slice(0, 16);

  return (
    <section className="reader">
      <div className="reader-head">
        <div className="reader-title">
          <Tag tool={s.tool} />
          <h1>{s.title}</h1>
        </div>
        <div className="reader-cwd">
          <IconFolder size={13} />
          <span>{s.cwd}</span>
          <span className="sep">·</span>
          <span>{s.message_count} 条消息</span>
          <span className="sep">·</span>
          <span>{timeStr}</span>
          {s.forked_from && (<><span className="sep">·</span><span>fork 自 {s.forked_from.slice(0, 8)}</span></>)}
        </div>
        <div className="reader-actions">
          <div className="cmd-pill">
            <span className="prompt">➜</span>
            <span className="cmd">{s.resume_command}</span>
          </div>
          <button className="btn" onClick={() => copyCommand(s.resume_command)}>
            <IconCopy size={13} /> 复制
          </button>
          <button className="btn primary" onClick={() => openTerminal(s)}>
            <IconTerminal size={13} /> 终端恢复
          </button>
        </div>
      </div>

      {loadingTranscript ? (
        <div className="skeleton">
          {[80, 60, 90, 50, 75].map((w, i) => (
            <div key={i} className="sk-line" style={{ width: `${w}%` }} />
          ))}
        </div>
      ) : (
        <div className="transcript">
          <div className="tcol">
            {transcript.map((m, i) => (
              <MessageView key={i} m={m} tool={s.tool} />
            ))}
          </div>
        </div>
      )}
    </section>
  );
}
