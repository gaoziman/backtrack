import { useEffect, useRef, useState } from "react";
import { useStore } from "../store";
import { Tag } from "./Tag";
import { MessageView } from "./MessageView";
import { FindBar } from "./FindBar";
import { IconCopy, IconDownload, IconFolder, IconTerminal } from "./icons";

export function Reader() {
  const { activeSession, transcript, loadingTranscript, copyCommand, openTerminal, openExport } = useStore();
  const transcriptRef = useRef<HTMLDivElement>(null);
  const [findOpen, setFindOpen] = useState(false);

  // 切换会话时关闭查找
  useEffect(() => {
    setFindOpen(false);
  }, [activeSession?.file_path]);

  // ⌘F / Ctrl+F 打开会话内查找（有活动会话时拦截浏览器默认查找）
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && (e.key === "f" || e.key === "F")) {
        if (!activeSession) return;
        e.preventDefault();
        setFindOpen(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [activeSession]);

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
          <button className="btn" onClick={() => openExport(s)}>
            <IconDownload size={13} /> 导出
          </button>
          <button className="btn primary" onClick={() => openTerminal(s)}>
            <IconTerminal size={13} /> 终端恢复
          </button>
        </div>
      </div>

      <div className="reader-main">
        {findOpen && (
          <FindBar
            containerRef={transcriptRef}
            recomputeKey={transcript}
            onClose={() => setFindOpen(false)}
          />
        )}
        {loadingTranscript ? (
          <div className="skeleton">
            {[80, 60, 90, 50, 75].map((w, i) => (
              <div key={i} className="sk-line" style={{ width: `${w}%` }} />
            ))}
          </div>
        ) : (
          <div className="transcript" ref={transcriptRef}>
            <div className="tcol">
              {transcript.map((m, i) => (
                <MessageView key={i} m={m} tool={s.tool} forceExpand={findOpen} />
              ))}
            </div>
          </div>
        )}
      </div>
    </section>
  );
}
