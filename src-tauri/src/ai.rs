//! AI 标题概括（可选功能，默认关闭）。
//!
//! 唯一触网模块：把会话前若干轮对话发给用户配置的 LLM 中转 API（Anthropic 原生格式），
//! 概括成一句精炼标题。纯函数（prompt 构造 / 响应解析 / 素材提取）可单测，
//! 网络调用隔离在 async 函数中。失败由调用方负责降级，绝不 panic。
use crate::models::{Message, Role};
use serde::{Deserialize, Serialize};
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

// ============================================================
//  AI 会话摘要（可选功能，默认关闭）— 与 AI 标题同构。
//  把会话前若干轮对话发给 LLM，概括成三段式结构化摘要。
// ============================================================

/// 摘要素材最大字符数（比标题更大，需要更多上下文）。
pub const SUMMARY_EXCERPT_MAX: usize = 6000;
/// 摘要响应 token 上限。
const SUMMARY_MAX_TOKENS: u32 = 512;
/// 摘要 gist 硬上限字符数。
const SUMMARY_GIST_MAX: usize = 80;
/// 关键结论最多条数。
const SUMMARY_CONCLUSION_MAX: usize = 5;
/// 涉及代码最多条数。
const SUMMARY_FILE_MAX: usize = 8;

const SUMMARY_SYSTEM_PROMPT: &str = "你是会话摘要助手。阅读这段开发对话后，只输出一个 JSON 对象，\
格式为 {\"gist\":\"一句话总结(不超过60字)\",\"conclusions\":[\"关键结论\",...最多5条],\"files\":[\"涉及的文件路径\",...]}。\
gist 概括对话核心；conclusions 列出关键决定/结论；files 列出对话中明确提到的代码文件路径(没有则空数组)。\
只输出 JSON 本身，不要解释、不要前缀、不要 markdown 代码块围栏。";

/// AI 摘要结构化结果（三段式）。
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct AiSummary {
    pub gist: String,
    pub conclusions: Vec<String>,
    pub files: Vec<String>,
}

impl AiSummary {
    /// 是否为空摘要（无任何可展示内容）。
    pub fn is_empty(&self) -> bool {
        self.gist.trim().is_empty() && self.conclusions.is_empty() && self.files.is_empty()
    }
}

/// 从会话消息提取摘要素材（纯函数）。与 excerpt_for_title 同构，仅预算更大。
pub fn excerpt_for_summary(messages: &[Message], max_chars: usize) -> String {
    excerpt_for_title(messages, max_chars)
}

/// 构造摘要请求体（纯函数）。
pub fn build_summary_request(model: &str, convo_excerpt: &str) -> Value {
    json!({
        "model": model,
        "max_tokens": SUMMARY_MAX_TOKENS,
        "system": SUMMARY_SYSTEM_PROMPT,
        "messages": [{ "role": "user", "content": convo_excerpt }]
    })
}

/// 把模型返回的文本解析为结构化摘要（纯函数）。
/// 先尝试 JSON 解析；失败则把整段文本降级为 gist。空文本 → None。
pub fn parse_summary_json(raw: &str) -> Option<AiSummary> {
    let text = strip_code_fence(raw.trim());
    if text.is_empty() {
        return None;
    }
    // 优先尝试结构化 JSON。
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        let gist = v
            .get("gist")
            .and_then(|g| g.as_str())
            .unwrap_or("")
            .trim()
            .chars()
            .take(SUMMARY_GIST_MAX)
            .collect::<String>();
        let conclusions = string_array(v.get("conclusions"), SUMMARY_CONCLUSION_MAX);
        let files = string_array(v.get("files"), SUMMARY_FILE_MAX);
        let s = AiSummary { gist, conclusions, files };
        if !s.is_empty() {
            return Some(s);
        }
    }
    // 降级：整段文本作为 gist。
    let gist: String = text.chars().take(SUMMARY_GIST_MAX).collect();
    Some(AiSummary { gist, conclusions: vec![], files: vec![] })
}

/// 去除 ```json ... ``` 之类的 markdown 代码块围栏（纯函数）。
fn strip_code_fence(s: &str) -> &str {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("```") {
        // 跳过可选语言标识行。
        let rest = rest.strip_prefix("json").unwrap_or(rest);
        let rest = rest.trim_start_matches('\n');
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim();
        }
        return rest.trim_end_matches('`').trim();
    }
    t
}

/// 从 JSON 值提取字符串数组，去空白/空项，限条数（纯函数）。
fn string_array(v: Option<&Value>, max: usize) -> Vec<String> {
    v.and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .take(max)
                .collect()
        })
        .unwrap_or_default()
}

/// 解析 Anthropic 响应，提取并结构化摘要（纯函数）。失败返回 None。
pub fn parse_summary_response(body: &Value) -> Option<AiSummary> {
    let content = body.get("content")?.as_array()?;
    let mut text = String::new();
    for part in content {
        if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
            text.push_str(t);
        }
    }
    parse_summary_json(&text)
}

/// 调用 LLM 生成摘要（async，触网）。失败返回 Err(脱敏信息)。
pub async fn request_summary(cfg: &AiConfig, convo_excerpt: &str) -> Result<AiSummary, String> {
    let client = http_client()?;
    let req = build_summary_request(&cfg.model, convo_excerpt);
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
    parse_summary_response(&body).ok_or_else(|| "AI 返回空摘要".to_string())
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

    // ---- AI 摘要测试 ----

    #[test]
    fn summary_excerpt_reuses_title_logic() {
        let msgs = vec![
            msg(Role::User, "重构导出模块", None),
            msg(Role::Assistant, "rm -rf x", Some("Bash")), // 工具调用，跳过
            msg(Role::Assistant, "改用策略模式", None),
        ];
        let ex = excerpt_for_summary(&msgs, 3000);
        assert!(ex.contains("重构导出模块"));
        assert!(ex.contains("改用策略模式"));
        assert!(!ex.contains("rm -rf x"), "工具调用应排除");
    }

    #[test]
    fn summary_request_shape() {
        let req = build_summary_request("claude-opus-4-8", "用户：你好");
        assert_eq!(req["model"], "claude-opus-4-8");
        assert_eq!(req["messages"][0]["role"], "user");
        assert_eq!(req["messages"][0]["content"], "用户：你好");
        assert!(req["system"].is_string());
        assert_eq!(req["max_tokens"], SUMMARY_MAX_TOKENS);
    }

    #[test]
    fn parse_summary_structured_json() {
        let raw = r#"{"gist":"修复登录态丢失","conclusions":["改用Cookie","加静默刷新"],"files":["src/auth.ts"]}"#;
        let s = parse_summary_json(raw).unwrap();
        assert_eq!(s.gist, "修复登录态丢失");
        assert_eq!(s.conclusions, vec!["改用Cookie", "加静默刷新"]);
        assert_eq!(s.files, vec!["src/auth.ts"]);
    }

    #[test]
    fn parse_summary_strips_code_fence() {
        let raw = "```json\n{\"gist\":\"加缓存\",\"conclusions\":[],\"files\":[]}\n```";
        let s = parse_summary_json(raw).unwrap();
        assert_eq!(s.gist, "加缓存");
    }

    #[test]
    fn parse_summary_degrades_to_gist_when_not_json() {
        let raw = "这是一段不是 JSON 的纯文本摘要";
        let s = parse_summary_json(raw).unwrap();
        assert_eq!(s.gist, "这是一段不是 JSON 的纯文本摘要");
        assert!(s.conclusions.is_empty());
        assert!(s.files.is_empty());
    }

    #[test]
    fn parse_summary_empty_is_none() {
        assert!(parse_summary_json("").is_none());
        assert!(parse_summary_json("   ").is_none());
    }

    #[test]
    fn parse_summary_filters_empty_array_items_and_limits() {
        let raw = r#"{"gist":"x","conclusions":["a","  ","b","c","d","e","f","g"],"files":[]}"#;
        let s = parse_summary_json(raw).unwrap();
        // 去空白项 + 限 5 条。
        assert_eq!(s.conclusions.len(), SUMMARY_CONCLUSION_MAX);
        assert!(!s.conclusions.iter().any(|c| c.trim().is_empty()));
    }

    #[test]
    fn parse_summary_gist_truncated() {
        let long = "字".repeat(200);
        let raw = format!(r#"{{"gist":"{}","conclusions":[],"files":[]}}"#, long);
        let s = parse_summary_json(&raw).unwrap();
        assert!(s.gist.chars().count() <= SUMMARY_GIST_MAX);
    }

    #[test]
    fn parse_summary_response_extracts() {
        let body = json!({
            "content": [{"type":"text","text":"{\"gist\":\"测试\",\"conclusions\":[],\"files\":[]}"}]
        });
        let s = parse_summary_response(&body).unwrap();
        assert_eq!(s.gist, "测试");
    }

    #[test]
    fn parse_summary_response_handles_missing() {
        assert!(parse_summary_response(&json!({})).is_none());
    }

    #[test]
    fn summary_is_empty_check() {
        assert!(AiSummary::default().is_empty());
        let s = AiSummary { gist: "x".into(), conclusions: vec![], files: vec![] };
        assert!(!s.is_empty());
    }
}
