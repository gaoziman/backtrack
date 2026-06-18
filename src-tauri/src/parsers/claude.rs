//! Claude Code `~/.claude/projects/<cwd>/<uuid>.jsonl` 解析器。
use crate::models::{Message, Role, SessionMeta, Tool};
use crate::parsers::{
    content_to_text, format_tool_input, is_command_invocation, is_greeting, is_system_noise,
    title_with_ai_fallback,
};
use serde_json::Value;
use std::path::Path;

fn role_of(role_str: &str) -> Role {
    if role_str == "assistant" {
        Role::Assistant
    } else {
        Role::User
    }
}

/// 把一条 message 的 content 拆成若干 Message（文本 / 工具调用 / 工具结果各成一块）。
fn emit_parts(
    content: &Value,
    role_str: &str,
    ts: &str,
    messages: &mut Vec<Message>,
    first_user: &mut Option<String>,
    first_sub: &mut Option<String>,
) {
    let track = |s: &str, fu: &mut Option<String>, fs: &mut Option<String>| {
        if !is_system_noise(s) {
            if fu.is_none() {
                *fu = Some(s.to_string());
            }
            // 实质句：非寒暄、非命令调用句（$analysis / 请你使用[$xxx](...) / /cmd）。
            if fs.is_none() && !is_greeting(s) && !is_command_invocation(s) {
                *fs = Some(s.to_string());
            }
        }
    };

    // 纯字符串内容
    if let Some(s) = content.as_str() {
        if s.trim().is_empty() {
            return;
        }
        let role = role_of(role_str);
        if matches!(role, Role::User) {
            track(s, first_user, first_sub);
        }
        messages.push(Message { role, text: s.to_string(), ts: ts.to_string(), tool_name: None });
        return;
    }

    let arr = match content.as_array() {
        Some(a) => a,
        None => return,
    };
    for part in arr {
        match part.get("type").and_then(|t| t.as_str()) {
            Some("text") => {
                let t = part.get("text").and_then(|t| t.as_str()).unwrap_or("");
                if t.trim().is_empty() {
                    continue;
                }
                let role = role_of(role_str);
                if matches!(role, Role::User) {
                    track(t, first_user, first_sub);
                }
                messages.push(Message { role, text: t.to_string(), ts: ts.to_string(), tool_name: None });
            }
            Some("tool_use") => {
                let name = part.get("name").and_then(|n| n.as_str()).unwrap_or("tool").to_string();
                let null = Value::Null;
                let input = part.get("input").unwrap_or(&null);
                let text = format_tool_input(&name, input);
                messages.push(Message { role: Role::Assistant, text, ts: ts.to_string(), tool_name: Some(name) });
            }
            Some("tool_result") => {
                let text = part.get("content").map(content_to_text).unwrap_or_default();
                messages.push(Message { role: Role::Tool, text, ts: ts.to_string(), tool_name: None });
            }
            _ => {}
        }
    }
}

/// 解析单个 Claude 会话文件。坏文件返回 None（由索引器跳过）。
pub fn parse_claude(path: &Path) -> Option<(SessionMeta, Vec<Message>)> {
    let raw = std::fs::read_to_string(path).ok()?;
    let id = path.file_stem()?.to_string_lossy().to_string();

    let mut cwd = String::new();
    let mut first_ts = String::new();
    let mut last_ts = String::new();
    let mut messages: Vec<Message> = Vec::new();
    let mut first_user_text: Option<String> = None;
    let mut first_substantive: Option<String> = None;

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue, // 容错：跳过坏行
        };

        if cwd.is_empty() {
            if let Some(c) = v.get("cwd").and_then(|c| c.as_str()) {
                cwd = c.to_string();
            }
        }
        if let Some(ts) = v.get("timestamp").and_then(|t| t.as_str()) {
            if first_ts.is_empty() {
                first_ts = ts.to_string();
            }
            last_ts = ts.to_string();
        }

        let typ = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if typ != "user" && typ != "assistant" {
            continue;
        }
        let msg = match v.get("message") {
            Some(m) => m,
            None => continue,
        };
        let role_str = msg.get("role").and_then(|r| r.as_str()).unwrap_or(typ);
        let null = Value::Null;
        let content = msg.get("content").unwrap_or(&null);
        let ts = v.get("timestamp").and_then(|t| t.as_str()).unwrap_or("");
        emit_parts(content, role_str, ts, &mut messages, &mut first_user_text, &mut first_substantive);
    }

    if messages.is_empty() {
        return None;
    }

    // 标题：优先 user 实质句 → user 首句 → AI 兜底（首条 assistant 文本，AI 常复述任务）。
    let title = title_with_ai_fallback(
        first_substantive.as_deref().or(first_user_text.as_deref()),
        &messages,
    );
    let meta = SessionMeta {
        resume_command: Tool::Claude.resume_command(&id),
        id,
        tool: Tool::Claude,
        cwd,
        file_path: path.to_string_lossy().to_string(),
        title,
        started_at: first_ts,
        updated_at: last_ts,
        message_count: messages.len(),
        forked_from: None,
        has_children: false,
    };
    Some((meta, messages))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_fixture(content: &str, name: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        dir
    }

    const SAMPLE: &str = r#"{"type":"user","sessionId":"sess1","cwd":"/Users/leo/proj","timestamp":"2026-06-16T01:00:00Z","message":{"role":"user","content":"hi"}}
{"type":"assistant","timestamp":"2026-06-16T01:00:05Z","message":{"role":"assistant","content":[{"type":"text","text":"你好，我来帮你"}]}}
{"type":"assistant","timestamp":"2026-06-16T01:00:10Z","message":{"role":"assistant","content":[{"type":"tool_use","name":"Bash","input":{"command":"ls"}}]}}
"#;

    #[test]
    fn parses_claude_session() {
        let dir = write_fixture(SAMPLE, "754a4be0.jsonl");
        let path = dir.path().join("754a4be0.jsonl");
        let (meta, msgs) = parse_claude(&path).expect("should parse");

        assert_eq!(meta.id, "754a4be0");
        assert_eq!(meta.tool, Tool::Claude);
        assert_eq!(meta.cwd, "/Users/leo/proj");
        assert_eq!(meta.title, "hi");
        assert_eq!(meta.message_count, 3);
        assert_eq!(meta.resume_command, "claude --resume '754a4be0'");
        assert_eq!(meta.started_at, "2026-06-16T01:00:00Z");
        assert_eq!(meta.updated_at, "2026-06-16T01:00:10Z");

        assert!(matches!(msgs[0].role, Role::User));
        assert!(matches!(msgs[1].role, Role::Assistant));
        assert_eq!(msgs[2].tool_name.as_deref(), Some("Bash"));
    }

    #[test]
    fn captures_tool_use_input() {
        let dir = write_fixture(SAMPLE, "x.jsonl");
        let (_, msgs) = parse_claude(&dir.path().join("x.jsonl")).unwrap();
        let bash = msgs.iter().find(|m| m.tool_name.as_deref() == Some("Bash")).unwrap();
        assert_eq!(bash.text, "ls"); // 入参的 command 被渲染出来了
    }

    #[test]
    fn splits_text_and_tool_use_in_one_message() {
        // 一条 assistant 消息同时含 text + tool_use → 拆成两块，正文不丢
        let content = r#"{"type":"assistant","timestamp":"t","message":{"role":"assistant","content":[{"type":"text","text":"我来跑个命令"},{"type":"tool_use","name":"Bash","input":{"command":"pwd"}}]}}
{"type":"user","cwd":"/p","timestamp":"t0","message":{"role":"user","content":"开始"}}"#;
        let dir = write_fixture(content, "y.jsonl");
        let (_, msgs) = parse_claude(&dir.path().join("y.jsonl")).unwrap();
        // text 块 + tool_use 块 + user 块 = 3
        assert_eq!(msgs.len(), 3);
        assert!(msgs.iter().any(|m| m.text == "我来跑个命令" && m.tool_name.is_none()));
        assert!(msgs.iter().any(|m| m.tool_name.as_deref() == Some("Bash") && m.text == "pwd"));
    }

    #[test]
    fn captures_tool_result() {
        let content = r#"{"type":"user","cwd":"/p","timestamp":"t","message":{"role":"user","content":[{"type":"tool_result","content":"Exit code 1"}]}}
{"type":"user","timestamp":"t","message":{"role":"user","content":"做点事"}}"#;
        let dir = write_fixture(content, "z.jsonl");
        let (_, msgs) = parse_claude(&dir.path().join("z.jsonl")).unwrap();
        let res = msgs.iter().find(|m| matches!(m.role, Role::Tool)).unwrap();
        assert_eq!(res.text, "Exit code 1");
    }

    #[test]
    fn skips_bad_lines() {
        let content = "not json\n".to_string() + SAMPLE;
        let dir = write_fixture(&content, "abc.jsonl");
        let path = dir.path().join("abc.jsonl");
        let (meta, _) = parse_claude(&path).expect("should still parse");
        assert_eq!(meta.message_count, 3);
    }

    #[test]
    fn empty_file_returns_none() {
        let dir = write_fixture("\n\n", "empty.jsonl");
        let path = dir.path().join("empty.jsonl");
        assert!(parse_claude(&path).is_none());
    }
}
