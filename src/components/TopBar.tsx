import { useRef } from "react";
import { useStore } from "../store";
import { IconBookmark, IconBookmarkFilled, IconChart, IconLogo, IconMoon, IconSearch, IconSettings } from "./icons";

const SINCE_OPTS = [["all", "全部"], ["7d", "近7天"], ["30d", "近30天"]] as const;
const ROLE_OPTS = [["all", "全部"], ["user", "我"], ["ai", "AI"]] as const;

export function TopBar() {
  const {
    query, setQuery, toolFilter, toggleTool, toggleTheme,
    searchRole, searchSince, setSearchRole, setSearchSince,
    searchCwd, setSearchCwd, projects, openSettings, openStats, statsOpen, closeStats,
    collectionsOpen, openCollections, closeCollections,
  } = useStore();
  const inputRef = useRef<HTMLInputElement>(null);

  // 暴露给 ⌘K
  (window as any).__focusSearch = () => inputRef.current?.focus();

  return (
    <header className="topbar">
      <div className="brand">
        <span className="logo"><IconLogo size={13} /></span>
        Backtrack <small>会话回溯</small>
      </div>

      <div className="search-wrap">
        <div className="search">
          <span className="icn"><IconSearch /></span>
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="搜索全部会话内容…  试试输入「旅迹」"
            autoComplete="off"
          />
          <kbd>⌘K</kbd>
        </div>
      </div>

      <div className="topbar-actions">
        {query.trim() && (
          <div className="filters">
            <div className="seg" role="group" aria-label="时间范围">
              {SINCE_OPTS.map(([v, l]) => (
                <button
                  key={v}
                  className={searchSince === v ? "on" : ""}
                  onClick={() => setSearchSince(v)}
                >
                  {l}
                </button>
              ))}
            </div>
            <div className="seg" role="group" aria-label="角色">
              {ROLE_OPTS.map(([v, l]) => (
                <button
                  key={v}
                  className={searchRole === v ? "on" : ""}
                  onClick={() => setSearchRole(v)}
                >
                  {l}
                </button>
              ))}
            </div>
            <select
              className="dir-filter"
              aria-label="按目录过滤"
              value={searchCwd}
              onChange={(e) => setSearchCwd(e.target.value)}
            >
              <option value="">全部目录</option>
              {projects.map((p) => (
                <option key={p.path} value={p.path}>
                  {p.display_name}
                </option>
              ))}
            </select>
          </div>
        )}
        <div className="tool-filter">
          {(["claude", "codex"] as const).map((t) => (
            <button
              key={t}
              className={toolFilter[t] ? "on" : "off"}
              onClick={() => toggleTool(t)}
            >
              <span className={`dot ${t}`} />
              {t === "claude" ? "Claude" : "Codex"}
            </button>
          ))}
        </div>
        <button
          className={`iconbtn${collectionsOpen ? " on" : ""}`}
          title="收藏"
          onClick={() => (collectionsOpen ? closeCollections() : openCollections())}
        >
          {collectionsOpen ? <IconBookmarkFilled size={15} /> : <IconBookmark size={15} />}
        </button>
        <button
          className={`iconbtn${statsOpen ? " on" : ""}`}
          title="使用统计"
          onClick={() => (statsOpen ? closeStats() : openStats())}
        >
          <IconChart size={15} />
        </button>
        <button className="iconbtn" title="AI 标题设置" onClick={openSettings}>
          <IconSettings size={15} />
        </button>
        <button className="iconbtn" title="切换明暗" onClick={() => toggleTheme()}>
          <IconMoon size={15} />
        </button>
      </div>
    </header>
  );
}
