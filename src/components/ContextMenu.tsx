import { useEffect } from "react";

export interface MenuItem {
  label: string;
  icon?: React.ReactNode;
  danger?: boolean;
  onClick: () => void;
}
export type MenuEntry = MenuItem | "divider";

export function ContextMenu({
  x,
  y,
  items,
  onClose,
}: {
  x: number;
  y: number;
  items: MenuEntry[];
  onClose: () => void;
}) {
  useEffect(() => {
    const close = () => onClose();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("mousedown", close);
    window.addEventListener("scroll", close, true);
    window.addEventListener("resize", close);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("scroll", close, true);
      window.removeEventListener("resize", close);
      window.removeEventListener("keydown", onKey);
    };
  }, [onClose]);

  // 防止贴边溢出
  const rows = items.length;
  const top = Math.max(8, Math.min(y, window.innerHeight - 12 - rows * 34));
  const left = Math.max(8, Math.min(x, window.innerWidth - 196));

  return (
    <div
      className="ctx-menu"
      style={{ top, left }}
      onMouseDown={(e) => e.stopPropagation()}
      onContextMenu={(e) => e.preventDefault()}
    >
      {items.map((it, i) =>
        it === "divider" ? (
          <div key={i} className="ctx-divider" />
        ) : (
          <div
            key={i}
            className={`ctx-item ${it.danger ? "danger" : ""}`}
            onClick={() => {
              it.onClick();
              onClose();
            }}
          >
            {it.icon && <span className="ci">{it.icon}</span>}
            {it.label}
          </div>
        )
      )}
    </div>
  );
}
