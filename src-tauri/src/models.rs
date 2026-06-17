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
    /// 该工具的 resume 命令模板。
    pub fn resume_command(&self, id: &str) -> String {
        match self {
            Tool::Claude => format!("claude --resume {}", id),
            Tool::Codex => format!("codex resume {}", id),
        }
    }
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
        assert_eq!(Tool::Claude.resume_command("abc"), "claude --resume abc");
        assert_eq!(Tool::Codex.resume_command("abc"), "codex resume abc");
    }

    #[test]
    fn session_meta_has_expected_fields() {
        let m = SessionMeta {
            id: "id1".into(), tool: Tool::Codex, cwd: "/x/y".into(),
            file_path: "/f".into(), title: "t".into(), started_at: "a".into(),
            updated_at: "b".into(), message_count: 2, forked_from: None,
            resume_command: "codex resume id1".into(),
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
