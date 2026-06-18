//! AI 标题概括（可选功能，默认关闭）。
//!
//! 唯一触网模块：把会话前若干轮对话发给用户配置的 LLM 中转 API（Anthropic 原生格式），
//! 概括成一句精炼标题。纯函数（prompt 构造 / 响应解析 / 素材提取）可单测，
//! 网络调用隔离在 async 函数中。失败由调用方负责降级，绝不 panic。
use crate::models::{Message, Role};
use serde_json::{json, Value};

/// 生成标题的最大字符数（清洗时硬上限）。
const TITLE_MAX_CHARS: usize = 24;
/// 网络超时秒数。
const TIMEOUT_SECS: u64 = 30;

const SYSTEM_PROMPT: &str = "你是会话标题概括助手。用一句不超过20字的中文短语概括这段对话的核心主题，\
只输出标题本身，不要引号、不要句号、不要解释、不要前缀。";

/// AI 配置（从 SQLite meta 表读取）。
#[derive(Clone, Debug, Default)]
pub struct AiConfig {
    pub enabled: bool,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

impl AiConfig {
    /// 是否具备可调用条件（启用 + key + 地址 + 模型俱全）。
    pub fn is_usable(&self) -> bool {
        self.enabled
            && !self.api_key.trim().is_empty()
            && !self.base_url.trim().is_empty()
            && !self.model.trim().is_empty()
    }
}

/// 从会话消息提取"前若干轮、限长"的概括素材（纯函数）。
/// 仅取 user/assistant 文本，排除工具调用与工具输出；按出现顺序拼接，截断到 max_chars。
pub fn excerpt_for_title(messages: &[Message], max_chars: usize) -> String {
    let mut out = String::new();
    for m in messages {
        if m.tool_name.is_some() || matches!(m.role, Role::Tool) {
            continue;
        }
        let who = match m.role {
            Role::User => "用户",
            Role::Assistant => "助手",
            Role::Tool => continue,
        };
        let text = m.text.trim();
        if text.is_empty() {
            continue;
        }
        // 单条也限长，避免单条超长消息吃满预算。
        let snippet: String = text.chars().take(600).collect();
        out.push_str(who);
        out.push('：');
        out.push_str(&snippet);
        out.push('\n');
        if out.chars().count() >= max_chars {
            break;
        }
    }
    out.chars().take(max_chars).collect()
}

/// 构造 Anthropic /v1/messages 请求体（纯函数）。
pub fn build_title_request(model: &str, convo_excerpt: &str) -> Value {
    json!({
        "model": model,
        "max_tokens": 64,
        "system": SYSTEM_PROMPT,
        "messages": [{ "role": "user", "content": convo_excerpt }]
    })
}

/// 解析 Anthropic 响应，提取并清洗标题（纯函数）。
/// 取 content[].text 拼接 → 去首尾空白/引号/句末标点 → 截断 ≤TITLE_MAX_CHARS。失败返回 None。
pub fn parse_title_response(body: &Value) -> Option<String> {
    let content = body.get("content")?.as_array()?;
    let mut text = String::new();
    for part in content {
        if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
            text.push_str(t);
        }
    }
    clean_ai_title(&text)
}

/// 清洗 AI 返回的标题文本（去引号/标记/截断）。空 → None。
pub fn clean_ai_title(raw: &str) -> Option<String> {
    let mut s = raw.trim().to_string();
    // 去成对引号包裹。
    for (open, close) in [('"', '"'), ('「', '」'), ('“', '”'), ('\'', '\'')] {
        if s.starts_with(open) && s.ends_with(close) && s.chars().count() >= 2 {
            let inner: String = s.chars().skip(1).take(s.chars().count() - 2).collect();
            s = inner.trim().to_string();
        }
    }
    // 取首行（防止模型多行输出）。
    if let Some(line) = s.lines().map(str::trim).find(|l| !l.is_empty()) {
        s = line.to_string();
    }
    // 去末尾句号类标点。
    s = s.trim_end_matches(|c: char| "。.！!".contains(c)).trim().to_string();
    if s.is_empty() {
        return None;
    }
    Some(s.chars().take(TITLE_MAX_CHARS).collect())
}

/// 调用 LLM 生成标题（async，唯一触网点）。失败返回 Err(脱敏信息)，调用方降级。
pub async fn request_title(cfg: &AiConfig, convo_excerpt: &str) -> Result<String, String> {
    let client = http_client()?;
    let req = build_title_request(&cfg.model, convo_excerpt);
    let resp = client
        .post(cfg.base_url.trim())
        .header("x-api-key", cfg.api_key.trim())
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&req)
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", redact(&e.to_string(), &cfg.api_key)))?;

    let status = resp.status();
    let body: Value = resp
        .json()
        .await
        .map_err(|e| format!("响应解析失败: {}", redact(&e.to_string(), &cfg.api_key)))?;

    if !status.is_success() {
        let msg = body
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("未知错误");
        return Err(format!("API {}: {}", status.as_u16(), redact(msg, &cfg.api_key)));
    }
    parse_title_response(&body).ok_or_else(|| "AI 返回空标题".to_string())
}

/// 测试连接：最小请求验证 key/地址/模型可用。
pub async fn test_connection(cfg: &AiConfig) -> Result<(), String> {
    if cfg.base_url.trim().is_empty() || cfg.api_key.trim().is_empty() || cfg.model.trim().is_empty()
    {
        return Err("请先填写 API 地址、密钥与模型".into());
    }
    request_title(cfg, "用户：你好\n助手：你好").await.map(|_| ())
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("HTTP 客户端初始化失败: {}", e))
}

/// 从错误信息里抹掉可能泄露的 api_key（防止 key 进日志/前端）。
fn redact(msg: &str, key: &str) -> String {
    if key.is_empty() {
        return msg.to_string();
    }
    msg.replace(key, "***")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Message, Role};

    fn msg(role: Role, text: &str, tool: Option<&str>) -> Message {
        Message { role, text: text.into(), ts: "t".into(), tool_name: tool.map(|s| s.into()) }
    }

    #[test]
    fn excerpt_skips_tools_and_limits() {
        let msgs = vec![
            msg(Role::User, "帮我重构登录模块", None),
            msg(Role::Assistant, "ls -la", Some("Bash")), // 工具调用，跳过
            msg(Role::Tool, "大段输出...", None),          // 工具结果，跳过
            msg(Role::Assistant, "好的，我先看代码", None),
        ];
        let ex = excerpt_for_title(&msgs, 3000);
        assert!(ex.contains("帮我重构登录模块"));
        assert!(ex.contains("好的，我先看代码"));
        assert!(!ex.contains("ls -la"), "工具调用应排除");
        assert!(!ex.contains("大段输出"), "工具结果应排除");
    }

    #[test]
    fn excerpt_respects_max_chars() {
        let long = "啊".repeat(5000);
        let msgs = vec![msg(Role::User, &long, None)];
        let ex = excerpt_for_title(&msgs, 100);
        assert!(ex.chars().count() <= 100);
    }

    #[test]
    fn build_request_shape() {
        let req = build_title_request("claude-opus-4-8", "用户：你好");
        assert_eq!(req["model"], "claude-opus-4-8");
        assert_eq!(req["messages"][0]["role"], "user");
        assert_eq!(req["messages"][0]["content"], "用户：你好");
        assert!(req["system"].is_string());
        assert_eq!(req["max_tokens"], 64);
    }

    #[test]
    fn parse_response_extracts_text() {
        let body = json!({
            "content": [{"type":"text","text":"重构登录认证流程"}]
        });
        assert_eq!(parse_title_response(&body).as_deref(), Some("重构登录认证流程"));
    }

    #[test]
    fn parse_response_handles_missing() {
        assert!(parse_title_response(&json!({})).is_none());
        assert!(parse_title_response(&json!({"content": []})).is_none());
    }

    #[test]
    fn clean_strips_quotes_and_punct() {
        assert_eq!(clean_ai_title("\"重构登录\""), Some("重构登录".into()));
        assert_eq!(clean_ai_title("「讨论缓存策略」"), Some("讨论缓存策略".into()));
        assert_eq!(clean_ai_title("修复空指针。"), Some("修复空指针".into()));
        assert_eq!(clean_ai_title("  探讨架构  "), Some("探讨架构".into()));
        assert_eq!(clean_ai_title(""), None);
        assert_eq!(clean_ai_title("   "), None);
    }

    #[test]
    fn clean_takes_first_line_and_truncates() {
        assert_eq!(clean_ai_title("标题甲\n多余说明文字"), Some("标题甲".into()));
        let long = "字".repeat(50);
        let t = clean_ai_title(&long).unwrap();
        assert!(t.chars().count() <= TITLE_MAX_CHARS);
    }

    #[test]
    fn config_usable_gating() {
        let mut c = AiConfig::default();
        assert!(!c.is_usable(), "默认不可用");
        c.enabled = true;
        c.base_url = "https://x/v1/messages".into();
        c.model = "claude-opus-4-8".into();
        assert!(!c.is_usable(), "缺 key 不可用");
        c.api_key = "sk-xxx".into();
        assert!(c.is_usable());
        c.enabled = false;
        assert!(!c.is_usable(), "关闭即不可用");
    }

    #[test]
    fn redact_removes_key() {
        let r = redact("error with sk-secret123 in url", "sk-secret123");
        assert!(!r.contains("sk-secret123"));
        assert!(r.contains("***"));
    }
}
