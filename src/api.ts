// 封装 Tauri invoke 调用（Tauri v2 自动把 camelCase 映射为 Rust snake_case）。
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Message, Project, ScanSummary, SearchHit, SearchRole, SessionMeta, Tool, ExportFormat,
} from "./types";

export const api = {
  scan: () => invoke<ScanSummary>("scan"),
  listProjects: () => invoke<Project[]>("list_projects"),
  listSessions: (cwd: string) => invoke<SessionMeta[]>("list_sessions", { cwd }),
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
  // 订阅后端「索引已更新」事件（文件监听自动刷新）。返回取消订阅函数。
  onIndexUpdated: (cb: (s: ScanSummary) => void): Promise<UnlistenFn> =>
    listen<ScanSummary>("index-updated", (e) => cb(e.payload)),
};
