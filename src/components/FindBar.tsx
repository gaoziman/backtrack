// 会话内查找浮层（⌘F）：输入 → 计数 → 上/下跳转 → 高亮，Esc 关闭。
import { useEffect, useRef, useState } from "react";
import { IconChevronDown, IconSearch, IconX } from "./icons";
import { useInSessionFind } from "./useInSessionFind";

export function FindBar({
  containerRef,
  recomputeKey,
  onClose,
}: {
  containerRef: React.RefObject<HTMLElement | null>;
  recomputeKey: unknown;
  onClose: () => void;
}) {
  const [q, setQ] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const { count, current, goNext, goPrev, supported } = useInSessionFind(
    containerRef,
    q,
    true,
    recomputeKey,
  );

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    } else if (e.key === "Enter" || e.key === "ArrowDown") {
      e.preventDefault();
      e.shiftKey ? goPrev() : goNext();
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      goPrev();
    }
  };

  return (
    <div className="find-bar" role="search">
      <span className="fi"><IconSearch size={13} /></span>
      <input
        ref={inputRef}
        value={q}
        onChange={(e) => setQ(e.target.value)}
        onKeyDown={onKey}
        placeholder={supported ? "在会话内查找…" : "当前环境不支持会话内高亮"}
        aria-label="在会话内查找"
        autoComplete="off"
        disabled={!supported}
      />
      <span className={`find-count ${q && count === 0 ? "none" : ""}`}>
        {q ? `${current}/${count}` : ""}
      </span>
      <button className="iconbtn sm" title="上一个 (⇧⏎)" onClick={goPrev} disabled={!count}>
        <span className="rot"><IconChevronDown size={14} /></span>
      </button>
      <button className="iconbtn sm" title="下一个 (⏎)" onClick={goNext} disabled={!count}>
        <IconChevronDown size={14} />
      </button>
      <button className="iconbtn sm" title="关闭 (Esc)" onClick={onClose}>
        <IconX size={14} />
      </button>
    </div>
  );
}
