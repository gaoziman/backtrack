import { useEffect, useRef, useState } from "react";
import { useStore } from "../store";
import { COLLECTION_COLORS, type CollectionColor } from "../types";
import { IconBookmarkFilled, IconCheck, IconPlus } from "./icons";

// compositionend 之后多久内的回车视为「输入法选字确认」而非提交（与 RenameDialog 一致）。
const IME_CONFIRM_WINDOW_MS = 80;

export function FavoriteDialog() {
  const {
    favDialogTarget, closeFavDialog, saveFavorite,
    collections, createCollection,
  } = useStore();

  // 已勾选的分类 id 集合。
  const [selected, setSelected] = useState<string[]>([]);
  // 新建分类输入。
  const [newName, setNewName] = useState("");
  const [newColor, setNewColor] = useState<CollectionColor>("coral");
  const inputRef = useRef<HTMLInputElement>(null);
  const composingRef = useRef(false);
  const compEndAtRef = useRef(-1e9);

  useEffect(() => {
    if (favDialogTarget) {
      setSelected(favDialogTarget.collection_ids ?? []);
      setNewName("");
      setNewColor("coral");
    }
  }, [favDialogTarget]);

  if (!favDialogTarget) return null;
  const t = favDialogTarget;

  const toggle = (id: string) =>
    setSelected((cur) => (cur.includes(id) ? cur.filter((x) => x !== id) : [...cur, id]));

  const addCollection = async () => {
    const name = newName.trim();
    if (!name) return;
    const c = await createCollection(name, newColor);
    if (c) {
      setSelected((cur) => [...cur, c.id]);
      setNewName("");
    }
  };

  // 保存：有勾选 → 收藏并归类；无勾选 → 仍收藏（未分类）。
  const save = () => saveFavorite(t.file_path, selected, true);
  // 取消收藏（彻底移除）。
  const unfavorite = () => saveFavorite(t.file_path, [], false);

  const inIme = (e: React.KeyboardEvent) =>
    composingRef.current ||
    e.nativeEvent.isComposing ||
    performance.now() - compEndAtRef.current < IME_CONFIRM_WINDOW_MS;

  return (
    <div
      className="scrim"
      onClick={(e) => {
        if (e.target === e.currentTarget) closeFavDialog();
      }}
    >
      <div className="modal" style={{ width: 460 }}>
        <div className="modal-head">
          <h2>
            <span style={{ display: "flex", color: "var(--gold)" }}>
              <IconBookmarkFilled size={16} />
            </span>
            收藏到分类
          </h2>
          <p>「{t.title}」· 选择一个或多个分类，留空则仅收藏不归类。</p>
        </div>
        <div className="modal-body">
          <div className="field-label">分类</div>
          <div className="chips">
            {collections.length === 0 && (
              <span style={{ color: "var(--text-lo)", fontSize: 12 }}>
                还没有分类，可在下方新建。
              </span>
            )}
            {collections.map((c) => {
              const on = selected.includes(c.id);
              return (
                <button
                  key={c.id}
                  className={`chip ${on ? "sel" : ""}`}
                  style={on ? { background: `var(--c-${c.color}-bg)`, color: `var(--c-${c.color})` } : undefined}
                  onClick={() => toggle(c.id)}
                >
                  <span className="dot" style={{ background: `var(--c-${c.color})` }} />
                  {c.name}
                  <span className="ck" style={{ opacity: on ? 1 : 0 }}>
                    <IconCheck size={13} />
                  </span>
                </button>
              );
            })}
          </div>

          <div className="field-label" style={{ marginTop: 18 }}>新建分类</div>
          <div className="new-coll">
            <input
              ref={inputRef}
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              onCompositionStart={() => { composingRef.current = true; }}
              onCompositionEnd={() => { composingRef.current = false; compEndAtRef.current = performance.now(); }}
              onKeyDown={(e) => {
                if (inIme(e)) return;
                if (e.key === "Enter") { e.preventDefault(); void addCollection(); }
              }}
              placeholder="分类名称…"
            />
            <div className="swatches">
              {COLLECTION_COLORS.map((col) => (
                <span
                  key={col}
                  className={`sw ${newColor === col ? "sel" : ""}`}
                  style={{ background: `var(--c-${col})` }}
                  onClick={() => setNewColor(col)}
                  title={col}
                />
              ))}
            </div>
            <button className="addbtn" onClick={() => void addCollection()} disabled={!newName.trim()}>
              <IconPlus size={13} />添加
            </button>
          </div>
        </div>
        <div className="modal-foot">
          {t.favorited && (
            <button className="reset-link" onClick={unfavorite}>取消收藏</button>
          )}
          <span style={{ flex: 1 }} />
          <button className="btn ghost" onClick={closeFavDialog}>取消</button>
          <button className="btn primary" onClick={save}>保存收藏</button>
        </div>
      </div>
    </div>
  );
}
