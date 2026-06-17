// 会话内查找：在已渲染的 transcript 文本上用 CSS Custom Highlight API 标记命中，
// 不修改 react-markdown 的 DOM（避免冲突 / 源码-渲染文本错配）。
import { useCallback, useEffect, useRef, useState } from "react";

const HL_ALL = "bt-find";
const HL_CUR = "bt-find-current";

// 运行环境是否支持 CSS Custom Highlight API（较旧 WebView 不支持 → 降级）。
const FIND_SUPPORTED =
  typeof (globalThis as any).Highlight !== "undefined" &&
  typeof CSS !== "undefined" &&
  !!(CSS as any).highlights;

/// 在 root 子树的文本节点里收集 query 的全部命中 Range（大小写不敏感）。
function collectRanges(root: HTMLElement, query: string): Range[] {
  const ranges: Range[] = [];
  const q = query.toLowerCase();
  if (!q) return ranges;
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
    acceptNode: (node) =>
      node.nodeValue && node.nodeValue.trim()
        ? NodeFilter.FILTER_ACCEPT
        : NodeFilter.FILTER_REJECT,
  });
  let node: Node | null;
  while ((node = walker.nextNode())) {
    const text = node.nodeValue!;
    const lower = text.toLowerCase();
    let from = 0;
    let idx: number;
    while ((idx = lower.indexOf(q, from)) !== -1) {
      const r = document.createRange();
      r.setStart(node, idx);
      r.setEnd(node, idx + query.length);
      ranges.push(r);
      from = idx + query.length;
    }
  }
  return ranges;
}

export function useInSessionFind(
  containerRef: React.RefObject<HTMLElement | null>,
  query: string,
  active: boolean,
  recomputeKey: unknown, // 变化时重算（transcript 切换 / 折叠展开生效后）
) {
  const [count, setCount] = useState(0);
  const [current, setCurrent] = useState(0); // 0 = 无；否则 1-based
  const rangesRef = useRef<Range[]>([]);

  const clearHighlights = useCallback(() => {
    if (!FIND_SUPPORTED) return;
    (CSS as any).highlights.delete(HL_ALL);
    (CSS as any).highlights.delete(HL_CUR);
  }, []);

  // 重算全部命中
  useEffect(() => {
    const root = containerRef.current;
    if (!active || !FIND_SUPPORTED || !root || !query.trim()) {
      rangesRef.current = [];
      setCount(0);
      setCurrent(0);
      clearHighlights();
      return;
    }
    // 等一帧，确保折叠展开后的布局已生效
    const id = requestAnimationFrame(() => {
      const ranges = collectRanges(root, query);
      rangesRef.current = ranges;
      setCount(ranges.length);
      if (ranges.length === 0) {
        setCurrent(0);
        clearHighlights();
        return;
      }
      (CSS as any).highlights.set(HL_ALL, new (globalThis as any).Highlight(...ranges));
      setCurrent(1);
    });
    return () => cancelAnimationFrame(id);
  }, [active, query, recomputeKey, containerRef, clearHighlights]);

  // 当前命中 → 单独高亮 + 居中滚动
  useEffect(() => {
    if (!FIND_SUPPORTED || !active) return;
    const ranges = rangesRef.current;
    if (current < 1 || current > ranges.length) {
      (CSS as any).highlights.delete(HL_CUR);
      return;
    }
    const r = ranges[current - 1];
    (CSS as any).highlights.set(HL_CUR, new (globalThis as any).Highlight(r.cloneRange()));
    const el =
      r.startContainer.nodeType === Node.TEXT_NODE
        ? r.startContainer.parentElement
        : (r.startContainer as HTMLElement);
    el?.scrollIntoView({ block: "center", behavior: "smooth" });
  }, [current, active, count]);

  // 失活 / 卸载清理
  useEffect(() => {
    if (!active) clearHighlights();
    return () => clearHighlights();
  }, [active, clearHighlights]);

  const goNext = useCallback(
    () => setCurrent((c) => (count === 0 ? 0 : (c % count) + 1)),
    [count],
  );
  const goPrev = useCallback(
    () => setCurrent((c) => (count === 0 ? 0 : ((c - 2 + count) % count) + 1)),
    [count],
  );

  return { count, current, goNext, goPrev, supported: FIND_SUPPORTED };
}
