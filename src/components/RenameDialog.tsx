import { useEffect, useRef, useState } from "react";
import { useStore } from "../store";
import { IconPencil } from "./icons";

// compositionend 之后多久内的回车/Esc 视为「输入法选字确认」而非真正提交。
// WKWebView 里确认回车紧跟 compositionend（间隔≈0），真正的提交回车一定远超此窗口。
const IME_CONFIRM_WINDOW_MS = 80;

export function RenameDialog() {
  const { renameTarget, closeRename, renameSession } = useStore();
  const [val, setVal] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const composingRef = useRef(false);
  const compEndAtRef = useRef(-1e9);

  useEffect(() => {
    if (renameTarget) {
      setVal(renameTarget.title);
      composingRef.current = false;
      compEndAtRef.current = -1e9;
      // 挂载后聚焦 + 全选当前标题
      requestAnimationFrame(() => {
        inputRef.current?.focus();
        inputRef.current?.select();
      });
    }
  }, [renameTarget]);

  if (!renameTarget) return null;
  const t = renameTarget;

  const save = () => renameSession(t.file_path, val);
  const reset = () => renameSession(t.file_path, "");

  // 是否处于输入法合成态（合成中 / 刚因选字结束合成）。
  // 兼容两类引擎：
  // - Chromium：合成中的 keydown isComposing=true（compositionend 在 keydown 之后）。
  // - WebKit/WKWebView：确认回车的 keydown 在 compositionend 之后、isComposing 已为 false，
  //   只能靠「紧跟 compositionend」的时间窗判定。
  const inIme = (e: React.KeyboardEvent) =>
    composingRef.current ||
    e.nativeEvent.isComposing ||
    performance.now() - compEndAtRef.current < IME_CONFIRM_WINDOW_MS;

  return (
    <div
      className="scrim"
      onClick={(e) => {
        if (e.target === e.currentTarget) closeRename();
      }}
    >
      <div className="modal" style={{ width: 430 }}>
        <div className="modal-head">
          <h2>
            <span style={{ display: "flex", color: "var(--accent)" }}>
              <IconPencil size={16} />
            </span>
            重命名标题
          </h2>
          <p>为这个会话起一个好记的名字。留空保存则恢复默认标题。</p>
        </div>
        <div className="modal-body">
          <label htmlFor="rename-input" className="sr-only">
            会话标题
          </label>
          <input
            id="rename-input"
            ref={inputRef}
            className="rename-input"
            value={val}
            onChange={(e) => setVal(e.target.value)}
            onCompositionStart={() => {
              composingRef.current = true;
            }}
            onCompositionEnd={() => {
              composingRef.current = false;
              compEndAtRef.current = performance.now();
            }}
            onKeyDown={(e) => {
              // 输入法选字/上屏阶段：回车只确认候选词、Esc 只取消合成，不触发保存/关闭。
              if (inIme(e)) return;
              if (e.key === "Enter") {
                e.preventDefault();
                save();
              }
              if (e.key === "Escape") {
                e.preventDefault();
                closeRename();
              }
            }}
            placeholder="输入新标题…"
          />
        </div>
        <div className="modal-foot">
          <button className="reset-link" onClick={reset}>
            恢复默认标题
          </button>
          <span style={{ flex: 1 }} />
          <button className="btn ghost" onClick={closeRename}>
            取消
          </button>
          <button className="btn primary" onClick={save}>
            保存
          </button>
        </div>
      </div>
    </div>
  );
}
