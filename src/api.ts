// 封装 Tauri invoke 调用（Tauri v2 自动把 camelCase 映射为 Rust snake_case）。
import { invoke } from "@tauri-apps/api/core";
import type { Message, Project, ScanSummary, SessionMeta, Tool } from "./types";

export const api = {
  scan: () => invoke<ScanSummary>("scan"),
  listProjects: () => invoke<Project[]>("list_projects"),
  listSessions: (cwd: string) => invoke<SessionMeta[]>("list_sessions", { cwd }),
  search: (query: string) => invoke<SessionMeta[]>("search", { query }),
  getTranscript: (filePath: string, tool: Tool) =>
    invoke<Message[]>("get_transcript", { filePath, tool }),
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
};
