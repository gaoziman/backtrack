import { useRef } from "react";
import { useStore } from "../store";
import { IconLogo, IconMoon, IconSearch } from "./icons";

export function TopBar() {
  const { query, setQuery, toolFilter, toggleTool, toggleTheme } = useStore();
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
        <button className="iconbtn" title="切换明暗" onClick={() => toggleTheme()}>
          <IconMoon size={15} />
        </button>
      </div>
    </header>
  );
}
