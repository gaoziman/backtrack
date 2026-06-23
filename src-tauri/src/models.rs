//! 跨前后端共享的数据类型（全部可序列化给前端 IPC）。
use serde::Serialize;

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Tool {
    Claude,
    Codex,
}

impl Tool {
    pub fn as_str(&self) -> &'static str {
        match self {
            Tool::Claude => "claude",
            Tool::Codex => "codex",
        }
    }
    pub fn from_str(s: &str) -> Option<Tool> {
        match s {
            "claude" => Some(Tool::Claude),
            "codex" => Some(Tool::Codex),
            _ => None,
        }
    }
    /// 该工具的 resume 命令模板。id 经 shell 单引号安全转义，防止特殊字符注入。
    pub fn resume_command(&self, id: &str) -> String {
        let safe = shell_single_quote(id);
        match self {
            Tool::Claude => format!("claude --resume {}", safe),
            Tool::Codex => format!("codex resume {}", safe),
        }
    }
}

/// 用单引号包裹字符串供 shell 使用，内部单引号转义为 `'\''`。
/// 例：`abc` → `'abc'`；`a'b` → `'a'\''b'`；`$(x)` → `'$(x)'`（不被 shell 展开）。
pub fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    Tool,
}

/// 单条会话的元数据（列表 + 卡片用）。
#[derive(Serialize, Clone, Debug)]
pub struct SessionMeta {
    pub id: String,
    pub tool: Tool,
    pub cwd: String,
    pub file_path: String,
    pub title: String,
    pub started_at: String,
    pub updated_at: String,
    pub message_count: usize,
    pub forked_from: Option<String>,
    /// 前端直接展示/复制的 resume 命令。
    pub resume_command: String,
    /// 派生字段（非入库）：是否有其它会话 fork 自本会话。
    /// 由 list_sessions 查询时计算，供前端判定是否显示 fork 谱系入口。
    /// 默认 false；序列化用 default 保证向后兼容、构造可省略。
    #[serde(default)]
    pub has_children: bool,
    /// 派生字段（非入库）：本会话是否已被收藏。
    /// 由 list_sessions/list_favorites 查询时 overlay；默认 false，构造可省略。
    #[serde(default)]
    pub favorited: bool,
    /// 派生字段（非入库）：本会话所属分类 id 列表（按分类 sort 升序）。
    /// 仅在需要时填充（list_favorites / session_by_path）；默认空。
    #[serde(default)]
    pub collection_ids: Vec<String>,
}

/// 收藏分类（用户自建）。`count` 为派生字段（该分类下的收藏数），非入库。
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Collection {
    pub id: String,
    pub name: String,
    /// 受控色板的 key（slate/coral/amber/green/teal/indigo/rose/brown）。
    pub color: String,
    pub sort: i64,
    #[serde(default)]
    pub count: usize,
}

/// 搜索命中：会话元数据 + 命中正文片段（仅标题命中时片段为 None）。
/// `#[serde(flatten)]` 使前端拿到 = SessionMeta 全字段 + 额外 `snippet`。
#[derive(Serialize, Clone, Debug)]
pub struct SearchHit {
    #[serde(flatten)]
    pub meta: SessionMeta,
    pub snippet: Option<String>,
}

/// 阅读器里的单条消息。
#[derive(Serialize, Clone, Debug)]
pub struct Message {
    pub role: Role,
    pub text: String,
    pub ts: String,
    pub tool_name: Option<String>,
}

/// 左栏目录（项目）节点。
#[derive(Serialize, Clone, Debug)]
pub struct Project {
    pub path: String,
    pub display_name: String,
    pub session_count: usize,
}

/// 单个工具的会话计数（统计面板工具占比用）。
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ToolCount {
    pub tool: Tool,
    pub count: usize,
}

/// 单个目录的会话计数（统计面板「最活跃目录」排行用）。
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct DirCount {
    pub cwd: String,
    pub display_name: String,
    pub count: usize,
}

/// 单月会话计数（统计面板「按月分布」柱状图用）。`month` 形如 `2026-06`。
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct MonthCount {
    pub month: String,
    pub count: usize,
}

/// 单日会话计数（统计面板「活跃热力图」用）。`day` 形如 `2026-06-16`。
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct DayCount {
    pub day: String,
    pub count: usize,
}

/// 全局使用统计（只读聚合，统计面板一次性取回全部所需数据）。
#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct StatsDto {
    /// 总会话数。
    pub total_sessions: usize,
    /// 总消息数（各会话 msg_count 之和）。
    pub total_messages: usize,
    /// 正文总字符数，前端按系数估算 token（不在后端硬编码系数，避免口径锁死）。
    pub total_body_chars: usize,
    /// 涉及的不同目录数。
    pub distinct_dirs: usize,
    /// fork 会话数（forked_from 非空）。
    pub fork_count: usize,
    /// 最早会话起始时间（ISO 字符串）；空库为 None。
    pub earliest: Option<String>,
    /// 最近会话更新时间（ISO 字符串）；空库为 None。
    pub latest: Option<String>,
    /// 按工具计数（claude / codex）。
    pub by_tool: Vec<ToolCount>,
    /// 按月计数，升序。
    pub by_month: Vec<MonthCount>,
    /// 按天计数，升序（前端取近 N 周渲染热力图）。
    pub by_day: Vec<DayCount>,
    /// 最活跃目录，降序（前端取 Top N）。
    pub top_dirs: Vec<DirCount>,
}

/// 把绝对 cwd 转成简洁展示名，取末尾 1-2 段。
pub fn display_name_for(cwd: &str) -> String {
    let parts: Vec<&str> = cwd.trim_end_matches('/').split('/').filter(|s| !s.is_empty()).collect();
    match parts.len() {
        0 => cwd.to_string(),
        1 => parts[0].to_string(),
        _ => format!("{} / {}", parts[parts.len() - 2], parts[parts.len() - 1]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_tool_lowercase() {
        let json = serde_json::to_string(&Tool::Claude).unwrap();
        assert_eq!(json, "\"claude\"");
    }

    #[test]
    fn resume_command_per_tool() {
        // 正常 UUID：包一对单引号，shell 行为等价、安全（回归 AC7）。
        assert_eq!(Tool::Claude.resume_command("abc"), "claude --resume 'abc'");
        assert_eq!(Tool::Codex.resume_command("abc"), "codex resume 'abc'");
        // 典型 UUID 形态
        assert_eq!(
            Tool::Claude.resume_command("5725ab12-cce7-4f1e-8820-60b1dd6dc906"),
            "claude --resume '5725ab12-cce7-4f1e-8820-60b1dd6dc906'"
        );
    }

    /// R1 安全：含 shell 元字符的 id 被安全转义，不会被执行。
    #[test]
    fn resume_command_escapes_shell_metachars() {
        // 命令替换 $() 被单引号包裹，不展开
        let cmd = Tool::Claude.resume_command("$(rm -rf /)");
        assert_eq!(cmd, "claude --resume '$(rm -rf /)'");
        // 内嵌单引号被正确转义
        let cmd = Tool::Codex.resume_command("a'b");
        assert_eq!(cmd, "codex resume 'a'\\''b'");
        // 反引号、分号
        let cmd = Tool::Claude.resume_command("x`whoami`;ls");
        assert_eq!(cmd, "claude --resume 'x`whoami`;ls'");
    }

    #[test]
    fn shell_single_quote_basics() {
        assert_eq!(shell_single_quote("abc"), "'abc'");
        assert_eq!(shell_single_quote(""), "''");
        assert_eq!(shell_single_quote("a'b"), "'a'\\''b'");
        assert_eq!(shell_single_quote("$(x)"), "'$(x)'");
    }

    #[test]
    fn session_meta_has_expected_fields() {
        let m = SessionMeta {
            id: "id1".into(), tool: Tool::Codex, cwd: "/x/y".into(),
            file_path: "/f".into(), title: "t".into(), started_at: "a".into(),
            updated_at: "b".into(), message_count: 2, forked_from: None,
            resume_command: "codex resume id1".into(), has_children: false,
            favorited: false, collection_ids: Vec::new(),
        };
        let v: serde_json::Value = serde_json::to_value(&m).unwrap();
        assert_eq!(v["tool"], "codex");
        assert_eq!(v["id"], "id1");
        assert_eq!(v["cwd"], "/x/y");
        assert_eq!(v["resume_command"], "codex resume id1");
    }

    #[test]
    fn display_name_takes_last_two() {
        assert_eq!(display_name_for("/Users/leo/coderspace/AI"), "coderspace / AI");
        assert_eq!(display_name_for("/solo"), "solo");
    }
}
