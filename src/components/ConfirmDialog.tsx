import { useStore } from "../store";
import { IconTrash } from "./icons";

function leaf(path: string): string {
  const parts = path.split("/").filter(Boolean);
  return parts[parts.length - 1] || path;
}

export function ConfirmDialog() {
  const { confirmDelete, cancelDelete, confirmDeleteProject } = useStore();
  if (!confirmDelete) return null;
  const p = confirmDelete;

  return (
    <div
      className="scrim"
      onClick={(e) => {
        if (e.target === e.currentTarget) cancelDelete();
      }}
    >
      <div className="modal" style={{ width: 430 }}>
        <div className="modal-head">
          <h2>
            <span style={{ color: "var(--danger)", display: "flex" }}>
              <IconTrash size={17} />
            </span>
            删除「{leaf(p.path)}」？
          </h2>
          <p>
            将把该目录下的 {p.session_count} 个会话移到 macOS 废纸篓，可随时从废纸篓恢复。
          </p>
        </div>
        <div className="modal-foot">
          <button className="btn ghost" onClick={cancelDelete}>
            取消
          </button>
          <button className="btn danger" onClick={confirmDeleteProject}>
            <IconTrash size={13} /> 移到废纸篓
          </button>
        </div>
      </div>
    </div>
  );
}
