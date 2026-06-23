// 封装 Tauri invoke 调用（Tauri v2 自动把 camelCase 映射为 Rust snake_case）。
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Message, Project, ScanSummary, SearchHit, SearchRole, SessionMeta, Tool, ExportFormat, ForkNode,
  AiConfigDto, AiSummary, StatsDto, Collection, CollectionColor,
} from "./types";

export const api = {
  scan: () => invoke<ScanSummary>("scan"),
  listProjects: () => invoke<Project[]>("list_projects"),
  listSessions: (cwd: string) => invoke<SessionMeta[]>("list_sessions", { cwd }),
  // 全局使用统计（统计面板，只读聚合，不触网）。
  stats: () => invoke<StatsDto>("stats"),
  search: (
    query: string,
    opts?: { role?: SearchRole; since?: string | null; tools?: Tool[]; cwd?: string | null },
  ) =>
    invoke<SearchHit[]>("search", {
      query,
      role: opts?.role ?? null,
      since: opts?.since ?? null,
      tools: opts?.tools ?? null,
      cwd: opts?.cwd ?? null,
    }),
  getTranscript: (filePath: string, tool: Tool) =>
    invoke<Message[]>("get_transcript", { filePath, tool }),
  // 导出单会话为 md/html；弹「另存为」对话框。返回保存路径，用户取消时为 null。
  exportSession: (
    filePath: string,
    tool: Tool,
    title: string,
    format: ExportFormat,
    includeTools: boolean,
  ) =>
    invoke<string | null>("export_session", {
      filePath,
      tool,
      title,
      format,
      includeTools,
    }),
  resumeInTerminal: (cwd: string, command: string, terminal: string) =>
    invoke<void>("resume_in_terminal", { cwd, command, terminal }),
  deleteProject: (cwd: string) => invoke<number>("delete_project", { cwd }),
  deleteSessions: (paths: string[]) => invoke<number>("delete_sessions", { paths }),
  hideProject: (cwd: string) => invoke<void>("hide_project", { cwd }),
  unhideProject: (cwd: string) => invoke<void>("unhide_project", { cwd }),
  listHidden: () => invoke<Project[]>("list_hidden"),
  listStarred: () => invoke<string[]>("list_starred"),
  setStar: (cwd: string, starred: boolean) => invoke<void>("set_star", { cwd, starred }),
  setStarredAll: (cwds: string[]) => invoke<void>("set_starred_all", { cwds }),
  revealInFinder: (path: string, reveal: boolean) =>
    invoke<void>("reveal_in_finder", { path, reveal }),
  // 重命名会话标题（空字符串=恢复默认）；返回生效后的标题。
  renameSession: (filePath: string, title: string) =>
    invoke<string>("rename_session", { filePath, title }),
  // 取会话所属 fork 谱系树（链顶为根）。
  forkTree: (filePath: string) => invoke<ForkNode>("fork_tree", { filePath }),
  // ---- AI 标题概括（可选功能，默认关闭）----
  getAiConfig: () => invoke<AiConfigDto>("get_ai_config"),
  setAiConfig: (enabled: boolean, baseUrl: string, apiKey: string, model: string) =>
    invoke<void>("set_ai_config", { enabled, baseUrl, apiKey, model }),
  testAiConnection: (baseUrl: string, apiKey: string, model: string) =>
    invoke<void>("test_ai_connection", { baseUrl, apiKey, model }),
  generateAiTitle: (filePath: string, tool: Tool, force: boolean) =>
    invoke<string | null>("generate_ai_title", { filePath, tool, force }),
  // ---- AI 会话摘要（可选功能，默认关闭）----
  // 只读缓存（选中会话时回显，不触网）。
  getAiSummary: (filePath: string) =>
    invoke<AiSummary | null>("get_ai_summary", { filePath }),
  // 按需生成（force=true 强制重新生成）。失败抛错，前端静默降级。
  generateAiSummary: (filePath: string, tool: Tool, force: boolean) =>
    invoke<AiSummary | null>("generate_ai_summary", { filePath, tool, force }),
  // ---- 收藏 + 分类（Collections）----
  listCollections: () => invoke<Collection[]>("list_collections"),
  createCollection: (name: string, color: CollectionColor) =>
    invoke<Collection>("create_collection", { name, color }),
  renameCollection: (id: string, name: string, color: CollectionColor) =>
    invoke<void>("rename_collection", { id, name, color }),
  deleteCollection: (id: string) => invoke<void>("delete_collection", { id }),
  reorderCollections: (ids: string[]) => invoke<void>("reorder_collections", { ids }),
  // 收藏/取消会话并设置所属分类（覆盖语义）。collectionIds 为空 + on=true = 仅收藏不归类。
  setFavorite: (filePath: string, collectionIds: string[], on: boolean) =>
    invoke<void>("set_favorite", { filePath, collectionIds, on }),
  // 收藏视图数据：collectionId=null 取全部收藏；query 非空叠加搜索。
  listFavorites: (collectionId: string | null, query: string | null) =>
    invoke<SessionMeta[]>("list_favorites", { collectionId, query }),
  // 订阅后端「索引已更新」事件（文件监听自动刷新）。返回取消订阅函数。
  onIndexUpdated: (cb: (s: ScanSummary) => void): Promise<UnlistenFn> =>
    listen<ScanSummary>("index-updated", (e) => cb(e.payload)),
};
