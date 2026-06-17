// 与 Rust 后端 Serialize 对应的前端类型。
export type Tool = "claude" | "codex";
export type Role = "user" | "assistant" | "tool";

export interface Project {
  path: string;
  display_name: string;
  session_count: number;
}

export interface SessionMeta {
  id: string;
  tool: Tool;
  cwd: string;
  file_path: string;
  title: string;
  started_at: string;
  updated_at: string;
  message_count: number;
  forked_from: string | null;
  resume_command: string;
}

export interface Message {
  role: Role;
  text: string;
  ts: string;
  tool_name: string | null;
}

/// 搜索命中 = 会话元数据 + 命中正文片段（仅标题命中时 snippet 为空）。
export interface SearchHit extends SessionMeta {
  snippet?: string | null;
}

export type SearchRole = "all" | "user" | "ai";
export type SearchSince = "all" | "7d" | "30d";
export type ExportFormat = "md" | "html";

export interface ScanSummary {
  total: number;
  claude: number;
  codex: number;
}
