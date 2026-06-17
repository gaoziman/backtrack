import React, { useState } from "react";
import { useStore } from "../store";
import type { Project, SearchHit, SessionMeta } from "../types";
import { ContextMenu, MenuEntry } from "./ContextMenu";
import {
  IconChevron, IconCopy, IconEye, IconEyeOff, IconFolder, IconPencil, IconRefresh, IconReveal,
  IconSliders, IconStar, IconStarFilled, IconTerminal, IconTrash,
} from "./icons";

function leafName(path: string): string {
  const parts = path.split("/").filter(Boolean);
  return parts[parts.length - 1] || path;
}

function relativeTime(iso: string): string {
  const t = Date.parse(iso);
  if (isNaN(t)) return "";
  const days = Math.floor((Date.now() - t) / 86_400_000);
  if (days <= 0) return "今天";
  if (days < 7) return `${days} 天`;
  if (days < 30) return `${Math.floor(days / 7)} 周`;
  if (days < 365) return `${Math.floor(days / 30)} 月`;
  return `${Math.floor(days / 365)} 年`;
}

function hl(text: string, q: string): React.ReactNode {
  if (!q) return text;
  const idx = text.toLowerCase().indexOf(q.toLowerCase());
  if (idx < 0) return text;
  return (
    <>
      {text.slice(0, idx)}
      <mark>{text.slice(idx, idx + q.length)}</mark>
      {hl(text.slice(idx + q.length), q)}
    </>
  );
}

type MenuState = { x: number; y: number; items: MenuEntry[] } | null;

export function Sidebar({ width }: { width: number }) {
  const {
    projects, hiddenProjects, sessionsByProject, expanded, loadingProject,
    toggleProject, toolFilter, searchResults, query, rescan, scanning,
    activeSession, selectSession,
    hideProject, unhideProject, requestDelete, deleteSessions, revealInFinder, copyCommand,
    starred, viewMode, setViewMode, openManage, toggleStar, openRename,
  } = useStore();

  const starredSet = new Set(starred);

  const [menu, setMenu] = useState<MenuState>(null);
  const [hiddenOpen, setHiddenOpen] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [anchor, setAnchor] = useState<string | null>(null);
  const [collapsedCwd, setCollapsedCwd] = useState<Set<string>>(new Set()); // 搜索态折叠的目录

  const toggleSearchGroup = (cwd: string) =>
    setCollapsedCwd((prev) => {
      const n = new Set(prev);
      n.has(cwd) ? n.delete(cwd) : n.add(cwd);
      return n;
    });

  const totalSessions = projects.reduce((a, p) => a + p.session_count, 0);
  const passTool = (s: SessionMeta) => toolFilter[s.tool];

  // ---- 会话点击：普通 / ⌘ 多选 / Shift 范围选 ----
  const onSessionClick = (e: React.MouseEvent, s: SessionMeta, list: SessionMeta[]) => {
    if (e.metaKey || e.ctrlKey) {
      setSelected((prev) => {
        const n = new Set(prev);
        n.has(s.file_path) ? n.delete(s.file_path) : n.add(s.file_path);
        return n;
      });
      setAnchor(s.file_path);
    } else if (e.shiftKey && anchor) {
      const ai = list.findIndex((x) => x.file_path === anchor);
      const ci = list.findIndex((x) => x.file_path === s.file_path);
      if (ai < 0 || ci < 0) {
        setSelected(new Set([s.file_path]));
      } else {
        const [lo, hi] = ai < ci ? [ai, ci] : [ci, ai];
        setSelected(new Set(list.slice(lo, hi + 1).map((x) => x.file_path)));
      }
    } else {
      setSelected(new Set([s.file_path]));
      setAnchor(s.file_path);
      selectSession(s);
    }
  };

  const onSessionContext = (e: React.MouseEvent, s: SessionMeta) => {
    e.preventDefault();
    let targets: string[];
    let delLabel: string;
    let single: boolean;
    if (selected.has(s.file_path) && selected.size > 1) {
      targets = [...selected];
      delLabel = `删除选中的 ${selected.size} 个会话`;
      single = false;
    } else {
      targets = [s.file_path];
      delLabel = "删除（移到废纸篓）";
      setSelected(new Set([s.file_path]));
      single = true;
    }
    setMenu({
      x: e.clientX, y: e.clientY,
      items: [
        { label: "复制 resume 命令", icon: <IconTerminal size={14} />, onClick: () => copyCommand(s.resume_command) },
        // 重命名为单会话操作，多选时隐藏
        ...(single
          ? [{ label: "重命名标题", icon: <IconPencil size={14} />, onClick: () => openRename(s) }]
          : []),
        { label: "在 Finder 中显示", icon: <IconReveal size={14} />, onClick: () => revealInFinder(s.file_path, true) },
        { label: "复制文件路径", icon: <IconCopy size={14} />, onClick: () => copyCommand(s.file_path) },
        "divider",
        { label: delLabel, icon: <IconTrash size={14} />, danger: true, onClick: () => { deleteSessions(targets); setSelected(new Set()); } },
      ],
    });
  };

  const renderSessions = (list: SessionMeta[], q: string) =>
    list.map((s) => {
      const on = selected.has(s.file_path) || activeSession?.file_path === s.file_path;
      return (
        <div
          key={s.file_path}
          className={`sess ${on ? "active" : ""}`}
          onClick={(e) => onSessionClick(e, s, list)}
          onContextMenu={(e) => onSessionContext(e, s)}
          title={s.title}
        >
          <span className={`tdot ${s.tool}`} />
          <span className="st">{hl(s.title, q)}</span>
          <span className="stime">{relativeTime(s.updated_at)}</span>
        </div>
      );
    });

  const projectMenu = (e: React.MouseEvent, p: Project): MenuState => {
    const isStar = starredSet.has(p.path);
    return {
      x: e.clientX, y: e.clientY,
      items: [
        {
          label: isStar ? "取消关注" : "关注",
          icon: isStar ? <IconStar size={14} /> : <IconStarFilled size={14} />,
          onClick: () => toggleStar(p),
        },
        { label: "在 Finder 中显示", icon: <IconReveal size={14} />, onClick: () => revealInFinder(p.path, false) },
        { label: "复制目录路径", icon: <IconCopy size={14} />, onClick: () => copyCommand(p.path) },
        { label: "隐藏（不删文件）", icon: <IconEyeOff size={14} />, onClick: () => hideProject(p) },
        "divider",
        { label: "删除（移到废纸篓）", icon: <IconTrash size={14} />, danger: true, onClick: () => requestDelete(p) },
      ],
    };
  };

  // 搜索命中：标题行 + 正文片段行（F1）
  const renderHits = (hits: SearchHit[], q: string) =>
    hits.map((s) => {
      const on = selected.has(s.file_path) || activeSession?.file_path === s.file_path;
      return (
        <div key={s.file_path} className="hit">
          <div
            className={`sess ${on ? "active" : ""}`}
            onClick={(e) => onSessionClick(e, s, hits)}
            onContextMenu={(e) => onSessionContext(e, s)}
            title={s.title}
          >
            <span className={`tdot ${s.tool}`} />
            <span className="st">{hl(s.title, q)}</span>
            <span className="stime">{relativeTime(s.updated_at)}</span>
          </div>
          {s.snippet && (
            <div
              className="sess-snippet"
              onClick={(e) => onSessionClick(e, s, hits)}
              title={s.snippet}
            >
              {hl(s.snippet, q)}
            </div>
          )}
        </div>
      );
    });

  // ---- 搜索态 ----
  const renderSearch = () => {
    const hits = (searchResults ?? []).filter(passTool);
    const byCwd = new Map<string, SearchHit[]>();
    for (const h of hits) {
      if (!byCwd.has(h.cwd)) byCwd.set(h.cwd, []);
      byCwd.get(h.cwd)!.push(h);
    }
    return (
      <>
        <div className="group-label">
          搜索 “<span className="hint">{query}</span>” · {hits.length} 命中
        </div>
        {hits.length === 0 ? (
          <div className="sess-empty">没有命中 · 试试更短的关键词，或调整上方工具/时间/角色过滤</div>
        ) : (
          [...byCwd.entries()].map(([cwd, sessions]) => {
            const open = !collapsedCwd.has(cwd);
            return (
              <div key={cwd}>
                <div
                  className={`proj ${open ? "open" : ""}`}
                  title={cwd}
                  onClick={() => toggleSearchGroup(cwd)}
                >
                  <span className="chev"><IconChevron size={12} /></span>
                  <span className="fi"><IconFolder /></span>
                  <span className="nm">{leafName(cwd)}</span>
                  <span className="ct">{sessions.length}</span>
                </div>
                {open && <div className="kids">{renderHits(sessions, query)}</div>}
              </div>
            );
          })
        )}
      </>
    );
  };

  // ---- 普通态：项目树 ----
  const renderTree = () => {
    // 关注置顶（全部模式）/ 只显示关注（关注模式）
    let display = projects;
    if (viewMode === "starred") {
      display = projects.filter((p) => starredSet.has(p.path));
    } else {
      const star = projects.filter((p) => starredSet.has(p.path));
      const rest = projects.filter((p) => !starredSet.has(p.path));
      display = [...star, ...rest];
    }

    return (
    <>
      <div className="tree-header">
        <span className="gl">项目</span>
        <span className="spacer" />
        <div className="seg">
          <button className={viewMode === "all" ? "on" : ""} onClick={() => setViewMode("all")}>全部</button>
          <button className={viewMode === "starred" ? "on" : ""} onClick={() => setViewMode("starred")}>
            <IconStarFilled size={11} />关注
          </button>
        </div>
        <span className="manage-btn" title="管理显示目录" onClick={openManage}><IconSliders size={15} /></span>
      </div>
      {scanning && projects.length === 0 && (
        <div style={{ padding: "2px 8px" }}>
          {[80, 65, 72, 58, 68].map((w, i) => (
            <div key={i} className="sk-mini" style={{ width: `${w}%`, height: 13, margin: "10px 4px" }} />
          ))}
        </div>
      )}
      {viewMode === "starred" && display.length === 0 && !scanning && (
        <div className="sess-empty" style={{ paddingLeft: 12 }}>
          还没关注任何目录 · 右键目录 → 关注，或点右上 ⚙ 管理
        </div>
      )}
      {display.map((p) => {
        const open = !!expanded[p.path];
        const loading = !!loadingProject[p.path];
        const isStar = starredSet.has(p.path);
        const sessions = (sessionsByProject[p.path] ?? []).filter(passTool);
        return (
          <div key={p.path}>
            <div
              className={`proj ${open ? "open" : ""}`}
              onClick={() => toggleProject(p.path)}
              onContextMenu={(e) => { e.preventDefault(); setMenu(projectMenu(e, p)); }}
              title={p.path}
            >
              <span className="chev"><IconChevron size={12} /></span>
              <span className="fi"><IconFolder /></span>
              <span className="nm">{leafName(p.path)}</span>
              <span
                className={`star ${isStar ? "on" : ""}`}
                title={isStar ? "取消关注" : "关注"}
                onClick={(e) => { e.stopPropagation(); toggleStar(p); }}
              >
                {isStar ? <IconStarFilled size={13} /> : <IconStar size={13} />}
              </span>
              <span className="ct">{p.session_count}</span>
            </div>
            {open && (
              <div className="kids">
                {loading ? (
                  <div className="sess-loading">
                    <div className="sk-mini" style={{ width: "70%" }} />
                    <div className="sk-mini" style={{ width: "55%" }} />
                  </div>
                ) : sessions.length === 0 ? (
                  <div className="sess-empty">无会话</div>
                ) : (
                  renderSessions(sessions, "")
                )}
              </div>
            )}
          </div>
        );
      })}

      {/* 已隐藏分组 */}
      {hiddenProjects.length > 0 && (
        <>
          <div className="group-label hidden-toggle" onClick={() => setHiddenOpen((o) => !o)}>
            <span className={`chev ${hiddenOpen ? "open" : ""}`}><IconChevron size={11} /></span>
            已隐藏 ({hiddenProjects.length})
          </div>
          {hiddenOpen &&
            hiddenProjects.map((hp) => (
              <div className="proj hidden-row" key={hp.path} title={hp.path}>
                <span className="chev" style={{ visibility: "hidden" }}><IconChevron size={12} /></span>
                <span className="fi"><IconFolder /></span>
                <span className="nm">{leafName(hp.path)}</span>
                <span className="ct">{hp.session_count}</span>
                <span className="unhide" title="取消隐藏" onClick={() => unhideProject(hp)}>
                  <IconEye size={13} />
                </span>
              </div>
            ))}
        </>
      )}
    </>
    );
  };

  return (
    <aside className="sidebar" style={{ width }}>
      <nav className="tree">{searchResults !== null ? renderSearch() : renderTree()}</nav>
      <div className="side-foot">
        <span className="blip" /> 已索引 {totalSessions} 会话 · {projects.length} 目录
        <span className={`rf ${scanning ? "spinning" : ""}`} title="刷新扫描" onClick={() => rescan()}>
          <IconRefresh size={14} />
        </span>
      </div>
      {menu && <ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => setMenu(null)} />}
    </aside>
  );
}
