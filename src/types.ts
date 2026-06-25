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
  /// 本会话是否已收藏（后端 list_sessions/list_favorites overlay）。
  favorited?: boolean;
  /// 本会话所属分类 id 列表（后端 list_favorites/session 详情填充）。
  collection_ids?: string[];
  /// 父会话 id：本会话为子代理时指向父会话；普通会话为 null。
  parent_id?: string | null;
  /// 本会话拥有的子代理数量（后端 list_sessions 计算）。
  subagent_count?: number;
}

/// 子代理摘要项（父会话折叠区列表用，与 Rust SubagentInfo 对应）。
export interface SubagentInfo {
  file_path: string;
  /// 友好名：.meta.json 的 description → agentType → 首句 → 兜底。
  name: string;
  /// agentType（如 "Explore"）；缺失为空串。
  agent_type: string;
  message_count: number;
  /// 文件体积（字节），前端按 KB 展示。
  size_bytes: number;
  started_at: string;
}

/// 收藏分类（与 Rust Collection 对应）。count 为该分类下收藏数。
export interface Collection {
  id: string;
  name: string;
  color: CollectionColor;
  sort: number;
  count?: number;
}

/// 受控分类色板 key（与 styles.css 的 --c-<key> 变量对应）。
export type CollectionColor =
  | "slate" | "coral" | "amber" | "green"
  | "teal" | "indigo" | "rose" | "brown";

/// 分类色板顺序（新建分类时的可选色）。
export const COLLECTION_COLORS: CollectionColor[] = [
  "slate", "coral", "amber", "green", "teal", "indigo", "rose", "brown",
];

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

/// 统计面板：单工具计数。
export interface ToolCount {
  tool: Tool;
  count: number;
}

/// 统计面板：单目录计数。
export interface DirCount {
  cwd: string;
  display_name: string;
  count: number;
}

/// 统计面板：单月计数（month 形如 2026-06）。
export interface MonthCount {
  month: string;
  count: number;
}

/// 统计面板：单日计数（day 形如 2026-06-16）。
export interface DayCount {
  day: string;
  count: number;
}

/// 全局使用统计（与 Rust StatsDto 对应，只读聚合）。
export interface StatsDto {
  total_sessions: number;
  total_messages: number;
  total_body_chars: number;
  distinct_dirs: number;
  fork_count: number;
  earliest: string | null;
  latest: string | null;
  by_tool: ToolCount[];
  by_month: MonthCount[];
  by_day: DayCount[];
  top_dirs: DirCount[];
}

/// AI 标题配置（从后端读取，key 脱敏为 has_key）。
export interface AiConfigDto {
  enabled: boolean;
  base_url: string;
  model: string;
  has_key: boolean;
}

/// AI 会话摘要（结构化三段式，与 Rust AiSummary 对应）。
export interface AiSummary {
  gist: string;
  conclusions: string[];
  files: string[];
}
