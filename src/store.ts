// 全局 UI 状态（zustand）。两栏可展开树模型。
import { create } from "zustand";
import { api } from "./api";
import type {
  Message, Project, SearchHit, SearchRole, SearchSince, SessionMeta, Tool,
} from "./types";

type Theme = "dark" | "light";

// 搜索防抖计时器（模块级，单实例）。
let searchTimer: number | undefined;

/// 把时间范围转成 ISO 下界；"all" → null。
function sinceISO(range: SearchSince): string | null {
  if (range === "all") return null;
  const days = range === "7d" ? 7 : 30;
  return new Date(Date.now() - days * 86_400_000).toISOString();
}

interface AppState {
  // 数据
  projects: Project[];
  hiddenProjects: Project[];
  sessionsByProject: Record<string, SessionMeta[]>; // 懒加载缓存（key = project.path）
  expanded: Record<string, boolean>;
  loadingProject: Record<string, boolean>;
  activeSession: SessionMeta | null;
  transcript: Message[];
  loadingTranscript: boolean;

  // 搜索
  query: string;
  searchResults: SearchHit[] | null; // null = 非搜索态
  searchRole: SearchRole;
  searchSince: SearchSince;

  // UI
  toolFilter: { claude: boolean; codex: boolean };
  theme: Theme;
  scanning: boolean;
  starred: string[]; // 关注的目录 cwd
  viewMode: "all" | "starred";
  manageOpen: boolean;

  // toast / modal
  toast: string | null;
  terminalModal: SessionMeta | null;
  confirmDelete: Project | null;

  // actions
  init: () => Promise<void>;
  rescan: () => Promise<void>;
  refreshLists: () => Promise<void>;
  toggleProject: (path: string) => Promise<void>;
  hideProject: (p: Project) => Promise<void>;
  unhideProject: (p: Project) => Promise<void>;
  requestDelete: (p: Project) => void;
  cancelDelete: () => void;
  confirmDeleteProject: () => Promise<void>;
  deleteSessions: (paths: string[]) => Promise<void>;
  revealInFinder: (path: string, reveal: boolean) => Promise<void>;
  selectSession: (s: SessionMeta) => Promise<void>;
  setQuery: (q: string) => void;
  setSearchRole: (r: SearchRole) => void;
  setSearchSince: (t: SearchSince) => void;
  runSearch: () => void;
  toggleTool: (t: Tool) => void;
  toggleTheme: () => void;
  toggleStar: (p: Project) => Promise<void>;
  setViewMode: (m: "all" | "starred") => void;
  openManage: () => void;
  closeManage: () => void;
  applyStarred: (cwds: string[]) => Promise<void>;
  copyCommand: (cmd: string) => void;
  openTerminal: (s: SessionMeta) => void;
  closeTerminal: () => void;
  doResume: (s: SessionMeta, terminal: string) => Promise<void>;
  showToast: (msg: string) => void;
}

export const useStore = create<AppState>((set, get) => ({
  projects: [],
  hiddenProjects: [],
  sessionsByProject: {},
  expanded: {},
  loadingProject: {},
  activeSession: null,
  transcript: [],
  loadingTranscript: false,
  query: "",
  searchResults: null,
  searchRole: "all",
  searchSince: "all",
  toolFilter: { claude: true, codex: true },
  theme: "light",
  scanning: false,
  starred: [],
  viewMode: "all",
  manageOpen: false,
  toast: null,
  terminalModal: null,
  confirmDelete: null,

  init: async () => {
    set({ scanning: true });
    try {
      await api.scan();
      await get().refreshLists();
      set({ scanning: false });
      const projects = get().projects;
      if (projects.length > 0) {
        // 自动展开第一个项目并选中其首个会话
        await get().toggleProject(projects[0].path);
        const first = get().sessionsByProject[projects[0].path]?.[0];
        if (first) await get().selectSession(first);
      }
    } catch (e) {
      set({ scanning: false });
      get().showToast(`扫描失败: ${e}`);
    }
  },

  refreshLists: async () => {
    const [projects, hiddenProjects, starred] = await Promise.all([
      api.listProjects(),
      api.listHidden(),
      api.listStarred(),
    ]);
    set({ projects, hiddenProjects, starred });
  },

  rescan: async () => {
    set({ scanning: true });
    try {
      const summary = await api.scan();
      await get().refreshLists();
      // 重新加载当前展开的项目会话，清空其余缓存
      const expandedPaths = Object.keys(get().expanded).filter((p) => get().expanded[p]);
      const cache: Record<string, SessionMeta[]> = {};
      for (const p of expandedPaths) {
        try {
          cache[p] = await api.listSessions(p);
        } catch {
          /* 跳过加载失败的项目 */
        }
      }
      set({ sessionsByProject: cache, scanning: false });
      get().showToast(`扫描完成 · ${summary.total} 个会话`);
    } catch (e) {
      set({ scanning: false });
      get().showToast(`扫描失败: ${e}`);
    }
  },

  toggleProject: async (path) => {
    const isOpen = !!get().expanded[path];
    if (isOpen) {
      set((s) => ({ expanded: { ...s.expanded, [path]: false } }));
      return;
    }
    set((s) => ({ expanded: { ...s.expanded, [path]: true } }));
    if (!get().sessionsByProject[path]) {
      set((s) => ({ loadingProject: { ...s.loadingProject, [path]: true } }));
      try {
        const sessions = await api.listSessions(path);
        set((s) => ({
          sessionsByProject: { ...s.sessionsByProject, [path]: sessions },
          loadingProject: { ...s.loadingProject, [path]: false },
        }));
      } catch (e) {
        set((s) => ({ loadingProject: { ...s.loadingProject, [path]: false } }));
        get().showToast(`加载会话失败: ${e}`);
      }
    }
  },

  selectSession: async (s) => {
    set({ activeSession: s, loadingTranscript: true, transcript: [] });
    try {
      const transcript = await api.getTranscript(s.file_path, s.tool);
      set({ transcript, loadingTranscript: false });
    } catch (e) {
      set({ loadingTranscript: false });
      get().showToast(`加载对话失败: ${e}`);
    }
  },

  setQuery: (q) => {
    set({ query: q });
    if (!q.trim()) {
      if (searchTimer) clearTimeout(searchTimer);
      set({ searchResults: null });
      return;
    }
    get().runSearch();
  },

  setSearchRole: (r) => {
    set({ searchRole: r });
    if (get().query.trim()) get().runSearch();
  },

  setSearchSince: (t) => {
    set({ searchSince: t });
    if (get().query.trim()) get().runSearch();
  },

  // 防抖 150ms 执行搜索，带竞态保护（仅当查询未变时应用结果）。
  runSearch: () => {
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = window.setTimeout(async () => {
      const q = get().query.trim();
      if (!q) {
        set({ searchResults: null });
        return;
      }
      try {
        const results = await api.search(q, {
          role: get().searchRole,
          since: sinceISO(get().searchSince),
        });
        if (get().query.trim() === q) set({ searchResults: results });
      } catch (e) {
        get().showToast(`搜索失败: ${e}`);
      }
    }, 150);
  },

  toggleTool: (t) =>
    set((s) => ({ toolFilter: { ...s.toolFilter, [t]: !s.toolFilter[t] } })),

  toggleTheme: () => {
    const next: Theme = get().theme === "dark" ? "light" : "dark";
    document.documentElement.dataset.theme = next;
    set({ theme: next });
  },

  toggleStar: async (p) => {
    const on = !get().starred.includes(p.path);
    try {
      await api.setStar(p.path, on);
      const starred = on
        ? [...get().starred, p.path]
        : get().starred.filter((c) => c !== p.path);
      set({ starred });
    } catch (e) {
      get().showToast(`操作失败: ${e}`);
    }
  },

  setViewMode: (m) => set({ viewMode: m }),
  openManage: () => set({ manageOpen: true }),
  closeManage: () => set({ manageOpen: false }),

  applyStarred: async (cwds) => {
    try {
      await api.setStarredAll(cwds);
      set({ starred: cwds, manageOpen: false });
      get().showToast(`已更新关注 · ${cwds.length} 个目录`);
    } catch (e) {
      get().showToast(`更新关注失败: ${e}`);
    }
  },

  copyCommand: (cmd) => {
    if (navigator.clipboard) navigator.clipboard.writeText(cmd).catch(() => {});
    const short = cmd.length > 46 ? cmd.slice(0, 46) + "…" : cmd;
    get().showToast(`已复制 ${short}`);
  },

  openTerminal: (s) => set({ terminalModal: s }),
  closeTerminal: () => set({ terminalModal: null }),

  doResume: async (s, terminal) => {
    set({ terminalModal: null });
    try {
      await api.resumeInTerminal(s.cwd, s.resume_command, terminal);
      get().showToast(`已在 ${terminal} 中恢复会话`);
    } catch (e) {
      get().showToast(`终端恢复失败: ${e}`);
    }
  },

  hideProject: async (p) => {
    try {
      await api.hideProject(p.path);
      // 若当前会话属于被隐藏目录，回空态
      if (get().activeSession?.cwd === p.path) set({ activeSession: null, transcript: [] });
      await get().refreshLists();
      get().showToast(`已隐藏 ${leaf(p.path)}`);
    } catch (e) {
      get().showToast(`隐藏失败: ${e}`);
    }
  },

  unhideProject: async (p) => {
    try {
      await api.unhideProject(p.path);
      await get().refreshLists();
      get().showToast(`已取消隐藏 ${leaf(p.path)}`);
    } catch (e) {
      get().showToast(`取消隐藏失败: ${e}`);
    }
  },

  requestDelete: (p) => set({ confirmDelete: p }),
  cancelDelete: () => set({ confirmDelete: null }),

  confirmDeleteProject: async () => {
    const p = get().confirmDelete;
    if (!p) return;
    set({ confirmDelete: null });
    try {
      const n = await api.deleteProject(p.path);
      // 清缓存 + 折叠态，若当前会话属于被删目录则回空态
      const { [p.path]: _drop, ...rest } = get().sessionsByProject;
      const exp = { ...get().expanded };
      delete exp[p.path];
      const patch: any = { sessionsByProject: rest, expanded: exp };
      if (get().activeSession?.cwd === p.path) {
        patch.activeSession = null;
        patch.transcript = [];
      }
      set(patch);
      await get().refreshLists();
      get().showToast(`已移到废纸篓 · ${n} 个会话`);
    } catch (e) {
      get().showToast(`删除失败: ${e}`);
    }
  },

  deleteSessions: async (paths) => {
    if (!paths.length) return;
    try {
      const n = await api.deleteSessions(paths);
      const del = new Set(paths);
      // 从所有项目缓存里移除被删会话
      const sbp = { ...get().sessionsByProject };
      for (const k of Object.keys(sbp)) {
        sbp[k] = sbp[k].filter((s) => !del.has(s.file_path));
      }
      const patch: any = { sessionsByProject: sbp };
      const active = get().activeSession;
      if (active && del.has(active.file_path)) {
        patch.activeSession = null;
        patch.transcript = [];
      }
      set(patch);
      await get().refreshLists();
      get().showToast(`已移到废纸篓 · ${n} 个会话`);
    } catch (e) {
      get().showToast(`删除失败: ${e}`);
    }
  },

  revealInFinder: async (path, reveal) => {
    try {
      await api.revealInFinder(path, reveal);
    } catch (e) {
      get().showToast(`打开 Finder 失败: ${e}`);
    }
  },

  showToast: (msg) => {
    set({ toast: msg });
    window.setTimeout(() => {
      if (get().toast === msg) set({ toast: null });
    }, 2400);
  },
}));

// 取路径叶子名（toast 用）
function leaf(path: string): string {
  const parts = path.split("/").filter(Boolean);
  return parts[parts.length - 1] || path;
}
