import { useEffect, useState } from "react";
import { useStore } from "../store";
import { IconCheck, IconPinFilled } from "./icons";

function leafName(path: string): string {
  const parts = path.split("/").filter(Boolean);
  return parts[parts.length - 1] || path;
}

export function ManageDirs() {
  const { manageOpen, closeManage, projects, starred, applyStarred } = useStore();
  const [checked, setChecked] = useState<Set<string>>(new Set());

  // 打开时用当前关注集合初始化
  useEffect(() => {
    if (manageOpen) setChecked(new Set(starred));
  }, [manageOpen, starred]);

  if (!manageOpen) return null;

  const toggle = (path: string) =>
    setChecked((prev) => {
      const n = new Set(prev);
      n.has(path) ? n.delete(path) : n.add(path);
      return n;
    });

  return (
    <div
      className="scrim"
      onClick={(e) => {
        if (e.target === e.currentTarget) closeManage();
      }}
    >
      <div className="modal" style={{ width: 460 }}>
        <div className="modal-head">
          <h2>
            <span style={{ color: "var(--accent)", display: "flex" }}>
              <IconPinFilled size={16} />
            </span>
            管理显示目录
          </h2>
          <p className="sub">勾选要关注的目录，切到「关注」视图时只显示它们。</p>
        </div>
        <div className="modal-body" style={{ paddingTop: 6, paddingBottom: 6 }}>
          <div className="manage-list">
            {projects.map((p) => {
              const on = checked.has(p.path);
              return (
                <div key={p.path} className="manage-row" onClick={() => toggle(p.path)} title={p.path}>
                  <span className={`cb ${on ? "on" : ""}`}>{on && <IconCheck size={11} />}</span>
                  <span className="nm">{leafName(p.path)}</span>
                  <span className="ct">{p.session_count}</span>
                </div>
              );
            })}
          </div>
        </div>
        <div className="modal-foot">
          <button className="btn ghost" onClick={closeManage}>取消</button>
          <button className="btn primary" onClick={() => applyStarred([...checked])}>
            应用（{checked.size}）
          </button>
        </div>
      </div>
    </div>
  );
}
