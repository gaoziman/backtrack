import { useState } from "react";
import { useStore } from "../store";
import type { SubagentInfo } from "../types";
import { IconAgent, IconChevron } from "./icons";

/// 子代理折叠块：挂在 Reader 头部（AI 摘要之下），可折叠。
/// 列出本次父会话调用的全部子代理；点击某行 drill-in 查看其完整对话。
/// 仅当父会话含子代理（subagents 非空）时由 Reader 渲染。
export function SubagentBlock({ subagents }: { subagents: SubagentInfo[] }) {
  const { selectSubagent } = useStore();
  const [open, setOpen] = useState(false);
  const n = subagents.length;

  return (
    <div className={`subagents${open ? " open" : ""}`}>
      <div className="sa-head" onClick={() => setOpen((v) => !v)}>
        <span className="ico"><IconAgent size={14} /></span>
        <span className="label">子代理</span>
        <span className="meta">· {n} 个 · 本次会话调用的并行代理</span>
        <span className="chev"><IconChevron size={13} /></span>
      </div>

      {open && (
        <div className="sa-body">
          {subagents.map((sa) => (
            <div
              key={sa.file_path}
              className="sa-row"
              role="button"
              tabIndex={0}
              onClick={() => selectSubagent(sa)}
              onKeyDown={(e) => {
                if (e.key === "Enter") selectSubagent(sa);
              }}
            >
              <span className="dot" data-type={sa.agent_type || "unknown"} />
              <span className="sa-name">{sa.name || "（子代理会话）"}</span>
              {sa.agent_type && <span className="sa-type">{sa.agent_type}</span>}
              <span className="sa-stat">
                {sa.message_count} 条 · {formatKB(sa.size_bytes)}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

/// 字节 → 紧凑 KB/MB 文本。
function formatKB(bytes: number): string {
  if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}
