import { useEffect, useRef } from "react";
import { useStore } from "../store";
import { Tag } from "./Tag";
import { IconFork } from "./icons";
import type { ForkNode, SessionMeta, Tool } from "../types";

// 时间格式化（复用 Reader 同款 MM-DD HH:mm）。
function fmtTime(iso?: string): string {
  if (!iso) return "";
  const m = iso.match(/(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2})/);
  return m ? `${m[2]}-${m[3]} ${m[4]}:${m[5]}` : iso.slice(0, 16);
}

// 单个树节点行（递归）。depth 控制缩进，isLast 控制连接线拐角。
function ForkNodeRow({
  node,
  depth,
  onPick,
}: {
  node: ForkNode;
  depth: number;
  onPick: (s: SessionMeta) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);

  // 当前节点：打开后自动滚入视野居中。
  useEffect(() => {
    if (node.is_current && ref.current) {
      const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
      ref.current.scrollIntoView({ block: "center", behavior: reduce ? "auto" : "smooth" });
    }
  }, [node.is_current]);

  const clickable = !node.missing;
  const onActivate = () => {
    if (node.missing) return;
    onPick(node as SessionMeta);
  };

  return (
    <div className="fork-branch">
      <div
        ref={ref}
        className={
          "fork-node-card" +
          (node.is_current ? " current" : "") +
          (node.missing ? " missing" : "")
        }
        style={{ marginLeft: depth * 22 }}
        role={clickable ? "button" : undefined}
        tabIndex={clickable ? 0 : undefined}
        aria-current={node.is_current ? "true" : undefined}
        onClick={onActivate}
        onKeyDown={(e) => {
          if (clickable && (e.key === "Enter" || e.key === " ")) {
            e.preventDefault();
            onActivate();
          }
        }}
        title={node.missing ? undefined : node.title}
      >
        <span className={"fork-dot" + (node.is_current ? " on" : "")} aria-hidden />
        {node.missing ? (
          <span className="fork-missing">（父会话不在本地）</span>
        ) : (
          <>
            <span className="fork-node-title">{node.title}</span>
            {node.is_current && <span className="fork-cur-badge">当前</span>}
            <span className="fork-node-meta">
              <Tag tool={(node.tool ?? "codex") as Tool} />
              <span>{fmtTime(node.updated_at)}</span>
              <span>· {node.message_count ?? 0}</span>
            </span>
          </>
        )}
      </div>
      {node.children.map((c, i) => (
        <ForkNodeRow
          key={c.file_path ?? `missing-${i}`}
          node={c}
          depth={depth + 1}
          onPick={onPick}
        />
      ))}
    </div>
  );
}

export function ForkTreeDialog() {
  const { forkTarget, forkTree, forkLoading, closeFork, selectSession } = useStore();
  if (!forkTarget) return null;

  const pick = (s: SessionMeta) => {
    closeFork();
    selectSession(s);
  };

  return (
    <div
      className="scrim"
      onClick={(e) => {
        if (e.target === e.currentTarget) closeFork();
      }}
      onKeyDown={(e) => {
        if (e.key === "Escape") closeFork();
      }}
    >
      <div className="modal" style={{ width: 560 }} role="dialog" aria-modal="true" aria-label="分支谱系">
        <div className="modal-head">
          <h2>
            <span style={{ display: "flex", color: "var(--accent)" }}>
              <IconFork size={17} />
            </span>
            分支谱系
          </h2>
          <p>这条对话的 fork 衍生关系，点击任意节点跳转阅读。</p>
        </div>
        <div className="modal-body">
          {forkLoading ? (
            <div className="skeleton">
              {[70, 55, 80].map((w, i) => (
                <div key={i} className="sk-line" style={{ width: `${w}%` }} />
              ))}
            </div>
          ) : forkTree ? (
            <div className="fork-tree">
              <ForkNodeRow node={forkTree} depth={0} onPick={pick} />
            </div>
          ) : null}
        </div>
        <div className="modal-foot">
          <button className="btn ghost" onClick={closeFork}>
            关闭
          </button>
        </div>
      </div>
    </div>
  );
}
