import { useLayoutEffect, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { Message, Tool } from "../types";
import { IconChevron, IconChevronDown, IconTool } from "./icons";

// 超过此高度的消息默认折叠（约 14 行）
const COLLAPSE_MAX = 320;

// 把 GFM 表格包一层可横向滚动 + 圆角边框的容器
const MD_COMPONENTS = {
  table: ({ node, ...props }: any) => (
    <div className="table-wrap">
      <table {...props} />
    </div>
  ),
};

function ToolCall({ m }: { m: Message }) {
  const [open, setOpen] = useState(false);
  const name = m.tool_name ?? "工具结果";
  return (
    <div className={`toolcall ${open ? "open" : ""}`}>
      <div className="toolcall-head" onClick={() => setOpen((o) => !o)}>
        <span className="chev"><IconChevron size={12} /></span>
        <IconTool size={13} />
        <span className="tname">{name}</span>
        <span className="tmeta">{open ? "点击收起" : "点击展开"}</span>
      </div>
      {open && (
        <div className="toolcall-body">{m.text || "（无输出内容）"}</div>
      )}
    </div>
  );
}

// 长消息默认折叠到 COLLAPSE_MAX，带渐隐 + 展开/收起按钮
function CollapsibleBody({
  isUser,
  dep,
  forceExpand,
  children,
}: {
  isUser: boolean;
  dep: string;
  forceExpand?: boolean;
  children: React.ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [overflow, setOverflow] = useState(false);
  const [collapsed, setCollapsed] = useState(true);

  useLayoutEffect(() => {
    const el = ref.current;
    if (el) setOverflow(el.scrollHeight > COLLAPSE_MAX + 28);
    setCollapsed(true); // 切换会话/内容变化时重置为折叠
  }, [dep]);

  // 查找态下强制展开，确保折叠区内的命中可见可滚动。
  const clamp = overflow && collapsed && !forceExpand;

  return (
    <>
      <div ref={ref} className={`msg-body ${isUser ? "user" : ""} ${clamp ? "clamped" : ""}`}>
        {children}
      </div>
      {overflow && !forceExpand && (
        <button className="collapse-btn" onClick={() => setCollapsed((c) => !c)}>
          {collapsed ? "展开全部" : "收起"}
          <span className={`cc ${collapsed ? "" : "up"}`}><IconChevronDown size={13} /></span>
        </button>
      )}
    </>
  );
}

export function MessageView({ m, tool, forceExpand }: { m: Message; tool: Tool; forceExpand?: boolean }) {
  // 工具调用 / 工具结果 → 折叠块
  if (m.tool_name || m.role === "tool") {
    return <ToolCall m={m} />;
  }

  // 空气泡（无正文、非工具）不渲染
  if (!m.text.trim()) return null;

  const isUser = m.role === "user";
  const who = isUser ? "你" : tool === "claude" ? "Claude" : "Codex";
  const avatarClass = isUser ? "avatar user" : `avatar ai-${tool}`;
  const ts = m.ts.match(/(\d{2}):(\d{2})/)?.[0] ?? "";

  return (
    <div className="msg">
      <div className="msg-head">
        <div className={avatarClass}>{isUser ? "你" : tool === "claude" ? "C" : "X"}</div>
        <span className="msg-who">{who}</span>
        <span className="msg-ts">{ts}</span>
      </div>
      <CollapsibleBody isUser={isUser} dep={m.text} forceExpand={forceExpand}>
        {isUser ? (
          m.text
        ) : (
          <ReactMarkdown remarkPlugins={[remarkGfm]} components={MD_COMPONENTS}>
            {m.text}
          </ReactMarkdown>
        )}
      </CollapsibleBody>
    </div>
  );
}
