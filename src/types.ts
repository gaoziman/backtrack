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
  /// 是否有其它会话 fork 自本会话（后端 list_sessions 计算）。供判定谱系入口。
  has_children?: boolean;
}

/// Fork 谱系树节点：会话元数据全字段（占位节点除外）+ 子节点。
/// missing=true 时为「父不在本地」占位节点，meta 字段缺失。
export interface ForkNode extends Partial<SessionMeta> {
  missing: boolean;
  is_current: boolean;
  children: ForkNode[];
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

/// AI 标题配置（从后端读取，key 脱敏为 has_key）。
export interface AiConfigDto {
  enabled: boolean;
  base_url: string;
  model: string;
  has_key: boolean;
}
