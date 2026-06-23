import { useEffect, useRef, useState } from "react";
import { useStore } from "../store";
import type { Collection } from "../types";
import {
  IconBookmark, IconBookmarkFilled, IconX, IconSearch,
  IconGrip, IconFolderPlus, IconPencil, IconTrash, IconPlay,
} from "./icons";
import { Tag } from "./Tag";

/// 全屏收藏视图：左分类列表（可拖拽排序）+ 右收藏会话流（支持搜索）。
export function CollectionsPanel() {
  const {
    collections, favSessions, favLoading, favActiveCollection, favQuery,
    closeCollections, setFavCollection, setFavQuery, reorderCollections,
    selectSession, openFavDialog, openTerminal, copyCommand,
  } = useStore();

  // 拖拽排序的本地顺序（拖拽中临时态）。
  const [order, setOrder] = useState<Collection[]>(collections);
  const dragId = useRef<string | null>(null);

  useEffect(() => { setOrder(collections); }, [collections]);

  const activeColl = collections.find((c) => c.id === favActiveCollection) || null;

  const onDrop = () => {
    dragId.current = null;
    reorderCollections(order.map((c) => c.id));
  };

  const reorderLocal = (overId: string) => {
    const from = order.findIndex((c) => c.id === dragId.current);
    const to = order.findIndex((c) => c.id === overId);
    if (from < 0 || to < 0 || from === to) return;
    const next = [...order];
    const [m] = next.splice(from, 1);
    next.splice(to, 0, m);
    setOrder(next);
  };

  const totalCount = collections.reduce((n, c) => n + (c.count ?? 0), 0);

  return (
    <div className="coll-view">
      <header className="coll-head">
        <div className="coll-title">
          <IconBookmarkFilled size={16} />收藏
        </div>
        <span style={{ flex: 1 }} />
        <button className="iconbtn" onClick={closeCollections} title="关闭收藏视图">
          <IconX size={16} />
        </button>
      </header>

      <div className="coll-body">
        {/* 左：分类列表（拖拽排序） */}
        <aside className="coll-side">
          <div className="coll-side-h">收藏分类</div>
          <div
            className={`cv-item ${favActiveCollection === null ? "on" : ""}`}
            onClick={() => setFavCollection(null)}
          >
            <IconBookmark size={15} />全部收藏
            <span className="cnt">{totalCount}</span>
          </div>
          {order.map((c) => (
            <div
              key={c.id}
              className={`cv-item drag ${favActiveCollection === c.id ? "on" : ""}`}
              draggable
              onClick={() => setFavCollection(c.id)}
              onDragStart={() => { dragId.current = c.id; }}
              onDragOver={(e) => { e.preventDefault(); if (dragId.current) reorderLocal(c.id); }}
              onDragEnd={onDrop}
            >
              <span className="grip" title="拖拽排序"><IconGrip size={14} /></span>
              <span className="dot" style={{ background: `var(--c-${c.color})` }} />
              {c.name}
              <span className="cnt">{c.count ?? 0}</span>
            </div>
          ))}
          <NewCollectionRow />
        </aside>

        {/* 右：收藏会话流 + 搜索 */}
        <main className="coll-main">
          <div className="coll-main-top">
            <div className="coll-main-title">
              {activeColl ? (
                <>
                  <span className="dot" style={{ background: `var(--c-${activeColl.color})` }} />
                  {activeColl.name}
                  <CollectionActions coll={activeColl} />
                </>
              ) : (
                <><IconBookmark size={15} />全部收藏</>
              )}
            </div>
            <div className="coll-search">
              <span className="si"><IconSearch size={14} /></span>
              <input
                value={favQuery}
                onChange={(e) => setFavQuery(e.target.value)}
                placeholder="在收藏中搜索…"
              />
            </div>
          </div>

          {favLoading ? (
            <div className="coll-grid">
              {[0, 1, 2, 3].map((i) => <div key={i} className="fav-card sk" />)}
            </div>
          ) : favSessions.length === 0 ? (
            <div className="coll-empty">
              <IconBookmark size={30} />
              <h3>{favQuery.trim() ? "没有匹配的收藏" : "这里还没有收藏"}</h3>
              <p>
                {favQuery.trim()
                  ? "换个关键词试试。"
                  : "在会话上点击星标，或右键选择「收藏到分类」即可加入。"}
              </p>
            </div>
          ) : (
            <div className="coll-grid">
              {favSessions.map((s) => (
                <div key={s.file_path} className="fav-card" onClick={() => { selectSession(s); closeCollections(); }}>
                  <button
                    className="cstar"
                    title="编辑收藏"
                    onClick={(e) => { e.stopPropagation(); openFavDialog(s); }}
                  >
                    <IconBookmarkFilled size={14} />
                  </button>
                  <div className="ct">{s.title}</div>
                  <div className="cm">
                    <Tag tool={s.tool} />
                    {s.message_count} 条
                  </div>
                  {(s.collection_ids?.length ?? 0) > 0 && (
                    <div className="ctags">
                      {s.collection_ids!.map((cid) => {
                        const c = collections.find((x) => x.id === cid);
                        if (!c) return null;
                        return (
                          <span
                            key={cid}
                            className="mini-chip"
                            style={{ background: `var(--c-${c.color}-bg)`, color: `var(--c-${c.color})` }}
                          >
                            <span className="d" style={{ background: `var(--c-${c.color})` }} />
                            {c.name}
                          </span>
                        );
                      })}
                    </div>
                  )}
                  <div className="fav-card-actions" onClick={(e) => e.stopPropagation()}>
                    <button className="mini-act" title="复制 resume 命令" onClick={() => copyCommand(s.resume_command)}>复制</button>
                    <button className="mini-act" title="终端恢复" onClick={() => openTerminal(s)}>
                      <IconPlay size={11} />恢复
                    </button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </main>
      </div>
    </div>
  );
}

/// 左栏底部「新建分类」内联行。
function NewCollectionRow() {
  const { createCollection } = useStore();
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState("");
  const ref = useRef<HTMLInputElement>(null);
  const composingRef = useRef(false);

  useEffect(() => { if (editing) requestAnimationFrame(() => ref.current?.focus()); }, [editing]);

  const submit = async () => {
    const n = name.trim();
    if (n) await createCollection(n, "slate");
    setName("");
    setEditing(false);
  };

  if (!editing) {
    return (
      <div className="cv-add" onClick={() => setEditing(true)}>
        <IconFolderPlus size={14} />新建分类
      </div>
    );
  }
  return (
    <div className="cv-add-edit">
      <input
        ref={ref}
        value={name}
        onChange={(e) => setName(e.target.value)}
        onCompositionStart={() => { composingRef.current = true; }}
        onCompositionEnd={() => { composingRef.current = false; }}
        onKeyDown={(e) => {
          if (composingRef.current || e.nativeEvent.isComposing) return;
          if (e.key === "Enter") { e.preventDefault(); void submit(); }
          if (e.key === "Escape") { setName(""); setEditing(false); }
        }}
        onBlur={() => void submit()}
        placeholder="分类名称…"
      />
    </div>
  );
}

/// 选中分类时的改名/删除入口。
function CollectionActions({ coll }: { coll: Collection }) {
  const { renameCollection, deleteCollection } = useStore();
  const [renaming, setRenaming] = useState(false);
  const [name, setName] = useState(coll.name);
  const ref = useRef<HTMLInputElement>(null);
  const composingRef = useRef(false);

  useEffect(() => { setName(coll.name); setRenaming(false); }, [coll.id, coll.name]);
  useEffect(() => { if (renaming) requestAnimationFrame(() => { ref.current?.focus(); ref.current?.select(); }); }, [renaming]);

  if (renaming) {
    const submit = async () => {
      const n = name.trim();
      if (n && n !== coll.name) await renameCollection(coll.id, n, coll.color);
      setRenaming(false);
    };
    return (
      <input
        ref={ref}
        className="coll-rename-input"
        value={name}
        onChange={(e) => setName(e.target.value)}
        onCompositionStart={() => { composingRef.current = true; }}
        onCompositionEnd={() => { composingRef.current = false; }}
        onKeyDown={(e) => {
          if (composingRef.current || e.nativeEvent.isComposing) return;
          if (e.key === "Enter") { e.preventDefault(); void submit(); }
          if (e.key === "Escape") { setName(coll.name); setRenaming(false); }
        }}
        onBlur={() => void submit()}
      />
    );
  }
  return (
    <span className="coll-act-btns">
      <button className="iconbtn sm" title="重命名分类" onClick={() => setRenaming(true)}>
        <IconPencil size={13} />
      </button>
      <button
        className="iconbtn sm danger"
        title="删除分类（收藏会降级为未分类，不丢失）"
        onClick={() => deleteCollection(coll.id)}
      >
        <IconTrash size={13} />
      </button>
    </span>
  );
}
