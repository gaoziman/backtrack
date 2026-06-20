// 全局 UI 状态（zustand）。两栏可展开树模型。
import { create } from "zustand";
import { api } from "./api";
import type {
  Message, Project, SearchHit, SearchRole, SearchSince, SessionMeta, Tool, ExportFormat, ForkNode,
  AiConfigDto, AiSummary,
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
  searchCwd: string; // "" = 全部目录

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
  renameTarget: SessionMeta | null;
  exportTarget: SessionMeta | null;
  forkTarget: SessionMeta | null; // 当前打开谱系面板的会话；null=关闭
  forkTree: ForkNode | null; // 已加载的 fork 树
  forkLoading: boolean;
  aiConfig: AiConfigDto | null; // AI 标题配置（null=未加载）
  settingsOpen: boolean;

  // AI 摘要（当前会话）
  aiSummary: AiSummary | null;      // 已生成/已缓存的摘要（null=无）
  aiSummaryLoading: boolean;        // 生成中
  aiSummaryError: string | null;    // 失败信息（null=无错误）

  // actions
  init: () => Promise<void>;
  rescan: () => Promise<void>;
  silentRefresh: () => Promise<void>;
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
  openRename: (s: SessionMeta) => void;
  closeRename: () => void;
  renameSession: (filePath: string, title: string) => Promise<void>;
  openExport: (s: SessionMeta) => void;
  closeExport: () => void;
  doExport: (format: ExportFormat, includeTools: boolean) => Promise<void>;
  openFork: (s: SessionMeta) => Promise<void>;
  closeFork: () => void;
  openSettings: () => void;
  closeSettings: () => void;
  loadAiConfig: () => Promise<void>;
  saveAiConfig: (cfg: { enabled: boolean; baseUrl: string; apiKey: string; model: string }) => Promise<void>;
  regenAiTitle: (s: SessionMeta) => Promise<void>;
  // AI 摘要：选中会话时回显缓存 / 手动生成（force=true 重新生成）。
  loadAiSummary: (s: SessionMeta) => Promise<void>;
  genAiSummary: (s: SessionMeta, force: boolean) => Promise<void>;
  setQuery: (q: string) => void;
  setSearchRole: (r: SearchRole) => void;
  setSearchSince: (t: SearchSince) => void;
  setSearchCwd: (cwd: string) => void;
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
  searchCwd: "",
  toolFilter: { claude: true, codex: true },
  theme: "light",
  scanning: false,
  starred: [],
  viewMode: "all",
  manageOpen: false,
  toast: null,
  terminalModal: null,
  confirmDelete: null,
  renameTarget: null,
  exportTarget: null,
  forkTarget: null,
  forkTree: null,
  forkLoading: false,
  aiConfig: null,
  settingsOpen: false,
  aiSummary: null,
  aiSummaryLoading: false,
  aiSummaryError: null,

  init: async () => {
    set({ scanning: true });
    try {
      get().loadAiConfig(); // 异步加载 AI 配置（不阻塞）
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

  // 文件监听触发的静默刷新：不置 scanning、不弹 toast、保留当前选中会话与展开态。
  // 区别于 rescan：后端已完成增量索引，前端仅刷新数据源 + 重载已展开项目会话。
  silentRefresh: async () => {
    await get().refreshLists();
    const expanded = Object.keys(get().expanded).filter((p) => get().expanded[p]);
    const active = get().activeSession;
    const toLoad = new Set(expanded);
    if (active) toLoad.add(active.cwd); // 便于校验当前会话是否仍存在
    const cache: Record<string, SessionMeta[]> = {};
    for (const p of toLoad) {
      try {
        cache[p] = await api.listSessions(p);
      } catch {
        /* 跳过加载失败的项目 */
      }
    }
    // 合并更新（保留未重载项目的旧缓存，避免折叠项目缓存丢失）。
    set((s) => ({ sessionsByProject: { ...s.sessionsByProject, ...cache } }));
    // 当前会话若已被删除（其文件不在刷新后的列表里）→ 回空态。
    if (active) {
      const list = cache[active.cwd];
      if (list && !list.some((x) => x.file_path === active.file_path)) {
        set({ activeSession: null, transcript: [] });
      }
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
    // 切换会话时重置摘要状态（避免串台）。
    set({
      activeSession: s, loadingTranscript: true, transcript: [],
      aiSummary: null, aiSummaryLoading: false, aiSummaryError: null,
    });
    try {
      const transcript = await api.getTranscript(s.file_path, s.tool);
      set({ transcript, loadingTranscript: false });
    } catch (e) {
      set({ loadingTranscript: false });
      get().showToast(`加载对话失败: ${e}`);
    }
    // AI 摘要：只读回显已缓存的摘要（不触网）。仅当仍是当前会话时落地。
    get().loadAiSummary(s);
    // AI 标题：开启且该会话标题撞车/无意义时，后台静默生成（不阻塞、失败不打断）。
    const cfg = get().aiConfig;
    if (cfg?.enabled && cfg.has_key && shouldGenAiTitle(s, get)) {
      api
        .generateAiTitle(s.file_path, s.tool, false)
        .then((title) => {
          if (title) applyAiTitle(set, get, s.file_path, title);
        })
        .catch(() => {
          /* 网络/key 失败 → 静默保持启发式标题 */
        });
    }
  },

  // 只读回显该会话已缓存的摘要（不触网）。竞态保护：落地前校验仍是当前会话。
  loadAiSummary: async (s) => {
    try {
      const summary = await api.getAiSummary(s.file_path);
      if (get().activeSession?.file_path === s.file_path) {
        set({ aiSummary: summary });
      }
    } catch {
      /* 读缓存失败 → 静默，当作无摘要 */
    }
  },

  // 手动生成摘要（force=true 重新生成）。含 loading/error 状态机。
  genAiSummary: async (s, force) => {
    set({ aiSummaryLoading: true, aiSummaryError: null });
    try {
      const summary = await api.generateAiSummary(s.file_path, s.tool, force);
      // 仅当仍停留在该会话时落地（防止生成期间切走导致串台）。
      if (get().activeSession?.file_path === s.file_path) {
        set({ aiSummary: summary, aiSummaryLoading: false });
      } else {
        set({ aiSummaryLoading: false });
      }
    } catch (e) {
      if (get().activeSession?.file_path === s.file_path) {
        set({ aiSummaryLoading: false, aiSummaryError: String(e) });
      } else {
        set({ aiSummaryLoading: false });
      }
    }
  },

  openRename: (s) => set({ renameTarget: s }),
  closeRename: () => set({ renameTarget: null }),

  // 重命名标题：调后端拿生效标题，就地更新列表/当前会话/搜索结果，不触发重扫。
  renameSession: async (filePath, title) => {
    try {
      const effective = await api.renameSession(filePath, title);
      const sbp = { ...get().sessionsByProject };
      for (const k of Object.keys(sbp)) {
        sbp[k] = sbp[k].map((s) =>
          s.file_path === filePath ? { ...s, title: effective } : s
        );
      }
      const patch: any = { sessionsByProject: sbp, renameTarget: null };
      const active = get().activeSession;
      if (active && active.file_path === filePath) {
        patch.activeSession = { ...active, title: effective };
      }
      const sr = get().searchResults;
      if (sr) {
        patch.searchResults = sr.map((s) =>
          s.file_path === filePath ? { ...s, title: effective } : s
        );
      }
      set(patch);
      get().showToast(title.trim() ? "已重命名" : "已恢复默认标题");
    } catch (e) {
      get().showToast(`重命名失败: ${e}`);
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

  setSearchCwd: (cwd) => {
    set({ searchCwd: cwd });
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
        const tf = get().toolFilter;
        const tools = (["claude", "codex"] as const).filter((t) => tf[t]);
        const results = await api.search(q, {
          role: get().searchRole,
          since: sinceISO(get().searchSince),
          tools,
          cwd: get().searchCwd || null,
        });
        if (get().query.trim() === q) set({ searchResults: results });
      } catch (e) {
        get().showToast(`搜索失败: ${e}`);
      }
    }, 150);
  },

  toggleTool: (t) => {
    set((s) => ({ toolFilter: { ...s.toolFilter, [t]: !s.toolFilter[t] } }));
    if (get().query.trim()) get().runSearch();
  },

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

  openExport: (s) => set({ exportTarget: s }),
  closeExport: () => set({ exportTarget: null }),
  doExport: async (format, includeTools) => {
    const s = get().exportTarget;
    if (!s) return;
    try {
      const path = await api.exportSession(s.file_path, s.tool, s.title, format, includeTools);
      set({ exportTarget: null });
      if (path) {
        get().showToast(`已导出 · ${leaf(path)}`);
        // 在 Finder 中定位导出的文件
        await get().revealInFinder(path, true);
      }
      // path 为 null = 用户取消另存为，静默。
    } catch (e) {
      set({ exportTarget: null });
      get().showToast(`导出失败: ${e}`);
    }
  },

  // 打开 fork 谱系面板：先开面板（占位 loading），再异步取树。
  openFork: async (s) => {
    set({ forkTarget: s, forkTree: null, forkLoading: true });
    try {
      const tree = await api.forkTree(s.file_path);
      // 竞态保护：仅当仍在查看同一会话时应用结果。
      if (get().forkTarget?.file_path === s.file_path) {
        set({ forkTree: tree, forkLoading: false });
      }
    } catch (e) {
      set({ forkTarget: null, forkTree: null, forkLoading: false });
      get().showToast(`加载谱系失败: ${e}`);
    }
  },
  closeFork: () => set({ forkTarget: null, forkTree: null, forkLoading: false }),

  openSettings: () => set({ settingsOpen: true }),
  closeSettings: () => set({ settingsOpen: false }),

  loadAiConfig: async () => {
    try {
      const aiConfig = await api.getAiConfig();
      set({ aiConfig });
    } catch {
      /* 读配置失败不影响主流程 */
    }
  },

  saveAiConfig: async (cfg) => {
    try {
      await api.setAiConfig(cfg.enabled, cfg.baseUrl, cfg.apiKey, cfg.model);
      await get().loadAiConfig();
      set({ settingsOpen: false });
      get().showToast(cfg.enabled ? "AI 标题已开启" : "AI 标题已关闭");
    } catch (e) {
      get().showToast(`保存设置失败: ${e}`);
    }
  },

  // 手动重新概括当前会话标题（强制覆盖缓存）。
  regenAiTitle: async (s) => {
    const cfg = get().aiConfig;
    if (!cfg?.enabled || !cfg.has_key) {
      get().showToast("请先在设置中开启 AI 标题");
      return;
    }
    get().showToast("正在概括标题…");
    try {
      const title = await api.generateAiTitle(s.file_path, s.tool, true);
      if (title) {
        applyAiTitle(set, get, s.file_path, title);
        get().showToast(`已概括：${title}`);
      } else {
        get().showToast("未生成标题");
      }
    } catch (e) {
      get().showToast(`概括失败: ${e}`);
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

// 判断该会话是否值得调 AI 生成标题：标题与其它会话撞车，或为寒暄/无意义短标题。
function shouldGenAiTitle(s: SessionMeta, get: () => AppState): boolean {
  const title = (s.title || "").trim();
  if (!title || title === "（无标题会话）") return true;
  // 寒暄/极短标题
  if (title.length <= 4) return true;
  // 撞车检测：在已加载的会话缓存里，同名标题 ≥ 2 个。
  const sbp = get().sessionsByProject;
  let count = 0;
  for (const k of Object.keys(sbp)) {
    for (const x of sbp[k]) {
      if (x.title === title) count++;
      if (count >= 2) return true;
    }
  }
  return false;
}

// 把 AI 标题就地写入各处会话副本（侧栏缓存 / 当前会话 / 搜索结果），并标记缓存已有。
function applyAiTitle(
  set: (partial: Partial<AppState> | ((s: AppState) => Partial<AppState>)) => void,
  get: () => AppState,
  filePath: string,
  title: string,
): void {
  const sbp = { ...get().sessionsByProject };
  for (const k of Object.keys(sbp)) {
    sbp[k] = sbp[k].map((s) => (s.file_path === filePath ? { ...s, title } : s));
  }
  const patch: Partial<AppState> = { sessionsByProject: sbp };
  const active = get().activeSession;
  if (active && active.file_path === filePath) {
    patch.activeSession = { ...active, title };
  }
  const sr = get().searchResults;
  if (sr) {
    patch.searchResults = sr.map((s) => (s.file_path === filePath ? { ...s, title } : s));
  }
  set(patch);
}
