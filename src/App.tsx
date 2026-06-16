import { useEffect, useRef, useState } from "react";
import { useStore } from "./store";
import { TopBar } from "./components/TopBar";
import { Sidebar } from "./components/Sidebar";
import { Reader } from "./components/Reader";
import { TerminalModal } from "./components/TerminalModal";
import { Toast } from "./components/Toast";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { ManageDirs } from "./components/ManageDirs";

export default function App() {
  const init = useStore((s) => s.init);
  const closeTerminal = useStore((s) => s.closeTerminal);
  const cancelDelete = useStore((s) => s.cancelDelete);
  const [sideW, setSideW] = useState(288);
  const dragging = useRef(false);

  useEffect(() => {
    init();
  }, [init]);

  // 全局快捷键：⌘K 聚焦搜索，Esc 关闭弹窗
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault();
        (window as any).__focusSearch?.();
      }
      if (e.key === "Escape") {
        closeTerminal();
        cancelDelete();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [closeTerminal, cancelDelete]);

  // 左栏拖拽改宽（220–380）
  const startResize = (e: React.MouseEvent) => {
    e.preventDefault();
    dragging.current = true;
    const startX = e.clientX;
    const startW = sideW;
    const move = (ev: MouseEvent) => {
      if (!dragging.current) return;
      setSideW(Math.min(380, Math.max(220, startW + ev.clientX - startX)));
    };
    const up = () => {
      dragging.current = false;
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      document.body.style.cursor = "";
    };
    document.body.style.cursor = "col-resize";
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  return (
    <div className="app">
      <TopBar />
      <div className="body">
        <Sidebar width={sideW} />
        <div className="resize" onMouseDown={startResize} />
        <Reader />
      </div>
      <TerminalModal />
      <ConfirmDialog />
      <ManageDirs />
      <Toast />
    </div>
  );
}
