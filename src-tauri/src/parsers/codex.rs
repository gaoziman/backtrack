//! Codex `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl` 解析器。
use crate::models::{Message, Role, SessionMeta, Tool};
use crate::parsers::{content_to_text, derive_title, format_tool_input, is_greeting, is_system_noise};
use serde_json::Value;
use std::path::Path;

/// 从 response_item 的 payload.content 数组拼出文本。
fn extract_message_text(payload: &Value) -> String {
    let mut text = String::new();
    if let Some(arr) = payload.get("content").and_then(|c| c.as_array()) {
        for part in arr {
            if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                text.push_str(t);
            }
        }
    }
    text
}

/// 解析单个 Codex 会话文件。坏文件返回 None。
pub fn parse_codex(path: &Path) -> Option<(SessionMeta, Vec<Message>)> {
    let raw = std::fs::read_to_string(path).ok()?;

    let mut id: Option<String> = None;
    let mut cwd = String::new();
    let mut forked_from: Option<String> = None;
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
            Err(_) => continue,
        };
        let typ = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let payload = v.get("payload").unwrap_or(&Value::Null);

        if let Some(ts) = v.get("timestamp").and_then(|t| t.as_str()) {
            if first_ts.is_empty() {
                first_ts = ts.to_string();
            }
            last_ts = ts.to_string();
        }
        let ts = v
            .get("timestamp")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();

        if typ == "session_meta" {
            if id.is_none() {
                id = payload.get("id").and_then(|i| i.as_str()).map(|s| s.to_string());
                if let Some(c) = payload.get("cwd").and_then(|c| c.as_str()) {
                    cwd = c.to_string();
                }
                forked_from = payload
                    .get("forked_from_id")
                    .and_then(|f| f.as_str())
                    .map(|s| s.to_string());
            }
            continue;
        }
        if typ != "response_item" {
            continue; // event_msg / token_count 等忽略
        }

        match payload.get("type").and_then(|t| t.as_str()).unwrap_or("") {
            "message" => {
                let role_str = payload.get("role").and_then(|r| r.as_str()).unwrap_or("");
                if role_str == "developer" {
                    continue; // 跳过环境注入噪声
                }
                let text = extract_message_text(payload);
                let role = if role_str == "assistant" {
                    Role::Assistant
                } else {
                    Role::User
                };
                if matches!(role, Role::User) && !is_system_noise(&text) {
                    if first_user_text.is_none() {
                        first_user_text = Some(text.clone());
                    }
                    if first_substantive.is_none() && !is_greeting(&text) {
                        first_substantive = Some(text.clone());
                    }
                }
                messages.push(Message { role, text, ts, tool_name: None });
            }
            "function_call" | "custom_tool_call" => {
                let name = payload
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("tool")
                    .to_string();
                // arguments 多为 JSON 字符串；custom 可能用 input
                let raw_args = payload.get("arguments").or_else(|| payload.get("input"));
                let input_val: Value = match raw_args {
                    Some(Value::String(s)) => {
                        serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.clone()))
                    }
                    Some(other) => other.clone(),
                    None => Value::Null,
                };
                let text = format_tool_input(&name, &input_val);
                messages.push(Message { role: Role::Assistant, text, ts, tool_name: Some(name) });
            }
            "function_call_output" | "custom_tool_call_output" => {
                let out = payload
                    .get("output")
                    .map(content_to_text)
                    .or_else(|| payload.get("result").map(content_to_text))
                    .unwrap_or_default();
                messages.push(Message { role: Role::Tool, text: out, ts, tool_name: None });
            }
            _ => {} // reasoning / web_search_call 等暂忽略
        }
    }

    let id = id.or_else(|| {
        path.file_stem().map(|s| {
            s.to_string_lossy()
                .rsplit('-')
                .take(5)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("-")
        })
    })?;

    if messages.is_empty() {
        return None;
    }

    let title = derive_title(first_substantive.as_deref().or(first_user_text.as_deref()));
    let meta = SessionMeta {
        resume_command: Tool::Codex.resume_command(&id),
        id,
        tool: Tool::Codex,
        cwd,
        file_path: path.to_string_lossy().to_string(),
        title,
        started_at: first_ts,
        updated_at: last_ts,
        message_count: messages.len(),
        forked_from,
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

    const SAMPLE: &str = r#"{"timestamp":"2026-03-16T03:59:09Z","type":"session_meta","payload":{"id":"019cf4cc-34ed","cwd":"/Users/leo/hub","forked_from_id":"prev-1"}}
{"timestamp":"2026-03-16T03:59:10Z","type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"<permissions instructions>"}]}}
{"timestamp":"2026-03-16T03:59:11Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"重构 admin 布局"}]}}
{"timestamp":"2026-03-16T03:59:20Z","type":"event_msg","payload":{"type":"task_started"}}
{"timestamp":"2026-03-16T03:59:25Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"好的，我来重构"}]}}
"#;

    #[test]
    fn parses_codex_session() {
        let dir = write_fixture(SAMPLE, "rollout-2026-03-16T03-59-09-019cf4cc-34ed.jsonl");
        let path = dir.path().join("rollout-2026-03-16T03-59-09-019cf4cc-34ed.jsonl");
        let (meta, msgs) = parse_codex(&path).expect("should parse");

        assert_eq!(meta.id, "019cf4cc-34ed");
        assert_eq!(meta.tool, Tool::Codex);
        assert_eq!(meta.cwd, "/Users/leo/hub");
        assert_eq!(meta.forked_from.as_deref(), Some("prev-1"));
        assert_eq!(meta.title, "重构 admin 布局");
        assert_eq!(meta.resume_command, "codex resume 019cf4cc-34ed");
        assert_eq!(meta.message_count, 2);
        assert!(matches!(msgs[0].role, Role::User));
        assert!(matches!(msgs[1].role, Role::Assistant));
    }

    #[test]
    fn ignores_event_and_developer() {
        let dir = write_fixture(SAMPLE, "r.jsonl");
        let (_, msgs) = parse_codex(&dir.path().join("r.jsonl")).unwrap();
        assert_eq!(msgs.len(), 2);
        assert!(msgs.iter().all(|m| !m.text.contains("permissions")));
    }

    #[test]
    fn renders_function_call_and_output() {
        let content = r#"{"timestamp":"t","type":"session_meta","payload":{"id":"u1","cwd":"/p"}}
{"timestamp":"t","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"看下时间"}]}}
{"timestamp":"t","type":"response_item","payload":{"type":"function_call","name":"shell_command","arguments":"{\"command\":\"date +%H\"}","call_id":"c1"}}
{"timestamp":"t","type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"22"}}"#;
        let dir = write_fixture(content, "tools.jsonl");
        let (_, msgs) = parse_codex(&dir.path().join("tools.jsonl")).unwrap();

        let call = msgs.iter().find(|m| m.tool_name.as_deref() == Some("shell_command")).unwrap();
        assert_eq!(call.text, "date +%H"); // arguments 里的 command 渲染出来
        let out = msgs.iter().find(|m| matches!(m.role, Role::Tool)).unwrap();
        assert_eq!(out.text, "22");
    }
}
