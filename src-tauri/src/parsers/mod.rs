//! 两种 jsonl 格式的解析器 + 公共辅助。
pub mod claude;
pub mod codex;

use serde_json::Value;
use std::path::Path;

/// 子代理 `.meta.json` 提炼出的展示信息。
pub struct SubagentMeta {
    /// 友好名候选（description 优先，否则 agentType；都缺则空串）。
    pub name: String,
    /// agentType（如 "Explore"）；缺失为空串。
    pub agent_type: String,
}

/// 读取子代理 jsonl 同名的 `.meta.json`（`agent-x.jsonl` → `agent-x.meta.json`），
/// 提炼 description / agentType。文件缺失或损坏 → None（由调用方降级到正文派生）。
///
/// 标题派生顺序（满足需求 F4）：description → agentType → （上层再退到正文首句 → 兜底）。
pub fn parse_subagent_meta(jsonl_path: &Path) -> Option<SubagentMeta> {
    // agent-x.jsonl → agent-x.meta.json
    let stem = jsonl_path.file_stem()?.to_string_lossy().to_string();
    let meta_path = jsonl_path.with_file_name(format!("{stem}.meta.json"));
    let raw = std::fs::read_to_string(&meta_path).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    let agent_type = v.get("agentType").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
    let desc = v.get("description").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
    // description 优先做友好名，缺失退到 agentType。
    let name = if !desc.is_empty() {
        desc
    } else {
        agent_type.clone()
    };
    Some(SubagentMeta { name, agent_type })
}

/// 把工具调用入参渲染成可读文本（Claude 的 tool_use.input / Codex 的 function_call.arguments）。
pub fn format_tool_input(name: &str, input: &Value) -> String {
    let lname = name.to_lowercase();
    // 命令类工具：Bash / shell_command / exec…
    if lname.contains("bash") || lname.contains("shell") || lname.contains("exec") {
        if let Some(cmd) = command_string(input) {
            return match input.get("description").and_then(|d| d.as_str()) {
                Some(d) if !d.is_empty() => format!("# {}\n{}", d, cmd),
                _ => cmd,
            };
        }
    }
    // 文件改动类：Edit / Write
    if name == "Edit" || name == "Write" {
        if let Some(fp) = input.get("file_path").and_then(|f| f.as_str()) {
            let mut out = fp.to_string();
            if let Some(o) = input.get("old_string").and_then(|s| s.as_str()) {
                out.push_str(&format!("\n\n--- 旧 ---\n{}", o));
            }
            let newv = input.get("new_string").or_else(|| input.get("content"));
            if let Some(n) = newv.and_then(|s| s.as_str()) {
                out.push_str(&format!("\n\n+++ 新 +++\n{}", n));
            }
            return out;
        }
    }
    serde_json::to_string_pretty(input).unwrap_or_else(|_| input.to_string())
}

fn command_string(input: &Value) -> Option<String> {
    match input.get("command") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Array(a)) => {
            let parts: Vec<String> =
                a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect();
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(" "))
            }
        }
        _ => None,
    }
}

/// tool_result / 工具输出 → 文本（支持 string 或 [{type:text,text}]）。
pub fn content_to_text(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    if let Some(arr) = v.as_array() {
        let mut out = String::new();
        for part in arr {
            if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                out.push_str(t);
            } else if let Some(s) = part.as_str() {
                out.push_str(s);
            }
        }
        return out;
    }
    if v.is_null() {
        return String::new();
    }
    v.to_string()
}

/// 判断一段文本是否为系统噪声（不应作为标题）。
pub fn is_system_noise(text: &str) -> bool {
    let t = text.trim_start();
    if t.is_empty() {
        return true;
    }
    let head: String = t.chars().take(80).collect();
    t.starts_with('<') // 含 <skill>/<subagent_notification>/<command->/<permissions> 等所有标签注入
        || t.starts_with("# AGENTS")
        || t.starts_with("Caveat")
        || t.starts_with("/resume")
        || t.starts_with("Base directory for this skill") // skill 启动注入文本
        || head.contains("system-reminder")
}

/// 判断是否为命令 / 技能调用句（语义上是"调用"而非"任务描述"，不该作为标题）。
/// 命中：裸命令 `$analysis`；调用模板 `请你使用 [$xxx](...)`；斜杠命令 `/xxx`。
pub fn is_command_invocation(text: &str) -> bool {
    let t = text.trim();
    // 裸命令：以 $ 紧跟字母开头（$analysis、$build…）
    if let Some(rest) = t.strip_prefix('$') {
        if rest.chars().next().is_some_and(|c| c.is_alphabetic()) {
            return true;
        }
    }
    // 斜杠命令：/resume、/clear…
    if let Some(rest) = t.strip_prefix('/') {
        if rest.chars().next().is_some_and(|c| c.is_alphabetic()) {
            return true;
        }
    }
    // 调用模板：请你使用 [$xxx](...) —— 链接文字以 $ 开头即视为命令调用
    let compact: String = t.split_whitespace().collect();
    if compact.starts_with("请你使用[$") || compact.starts_with("[$") {
        return true;
    }
    false
}

/// 把一条文本清洗为干净的标题候选：
/// 剥 Markdown 链接 `[文字](url)`→`文字`、行内代码、行首标记，去纯路径，归一空白。
/// 返回清洗后文本；清洗后为空 / 纯路径 / 命令调用 → None（这条不适合做标题）。
pub fn clean_title_text(text: &str) -> Option<String> {
    if is_command_invocation(text) {
        return None;
    }
    // 取首个非空行做标题来源（标题是单行的）。
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    // 去行首 Markdown 标记：# ## > - * 及其后空白。
    let line = strip_leading_md_marks(line);
    // 剥 Markdown 链接与行内代码。
    let unwrapped = unwrap_md(&line);
    let s = unwrapped.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.is_empty() {
        return None;
    }
    // 纯路径（整句就是 /xxx 或 ~/xxx，无空格）→ 无信息量。
    if (s.starts_with('/') || s.starts_with("~/")) && !s.contains(' ') {
        return None;
    }
    Some(s)
}

/// 去掉行首的 Markdown 标记（`#`/`>`/`-`/`*` 及随后空白），可叠加。
fn strip_leading_md_marks(line: &str) -> String {
    let mut s = line.trim_start();
    loop {
        let stripped = s
            .strip_prefix("# ")
            .or_else(|| s.strip_prefix("## "))
            .or_else(|| s.strip_prefix("### "))
            .or_else(|| s.strip_prefix("> "))
            .or_else(|| s.strip_prefix("- "))
            .or_else(|| s.strip_prefix("* "));
        match stripped {
            Some(rest) => s = rest.trim_start(),
            None => break,
        }
    }
    s.to_string()
}

/// 手写扫描剥离 Markdown 链接 `[文字](url)`→`文字` 与行内代码 `` `x` ``→`x`（无 regex 依赖）。
fn unwrap_md(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '[' {
            // 尝试匹配 [text](url)
            if let Some((text, next)) = parse_md_link(&chars, i) {
                out.push_str(&text);
                i = next;
                continue;
            }
        }
        if c == '`' {
            // 行内代码：吃到下一个 `，保留中间内容
            if let Some(close) = chars[i + 1..].iter().position(|&x| x == '`') {
                let inner: String = chars[i + 1..i + 1 + close].iter().collect();
                out.push_str(&inner);
                i = i + 1 + close + 1;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// 从 `chars[start]=='['` 处尝试解析 `[text](url)`，成功返回 (text, 下一索引)。
fn parse_md_link(chars: &[char], start: usize) -> Option<(String, usize)> {
    debug_assert_eq!(chars[start], '[');
    let close_br = chars[start + 1..].iter().position(|&c| c == ']')? + start + 1;
    // ] 后必须紧跟 (
    if chars.get(close_br + 1) != Some(&'(') {
        return None;
    }
    let close_par = chars[close_br + 2..].iter().position(|&c| c == ')')? + close_br + 2;
    let text: String = chars[start + 1..close_br].iter().collect();
    Some((text, close_par + 1))
}

/// 判断是否为无信息量的寒暄（不适合做标题）。
pub fn is_greeting(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    let t = t.trim_end_matches(|c: char| "！!。.~～,，".contains(c));
    matches!(
        t,
        "hi" | "hello" | "hey" | "yo" | "你好" | "您好" | "在吗" | "哈喽" | "test" | "ping"
    )
}

/// 生成标题，user 侧候选提炼不出时用首条 AI 文本兜底（AI 常复述任务，比"无标题"有用）。
/// `messages` 用于查找首条 assistant 文本。
pub fn title_with_ai_fallback(
    user_candidate: Option<&str>,
    messages: &[crate::models::Message],
) -> String {
    let from_user = derive_title(user_candidate);
    if from_user != "（无标题会话）" {
        return from_user;
    }
    // 兜底：首条 assistant 文本（非工具调用）。
    let ai = messages.iter().find(|m| {
        matches!(m.role, crate::models::Role::Assistant) && m.tool_name.is_none() && !m.text.trim().is_empty()
    });
    match ai.and_then(|m| clean_title_text(&m.text)) {
        Some(s) => truncate_title(&s, 60),
        None => "（无标题会话）".to_string(),
    }
}

/// 从候选文本生成简洁标题：清洗（剥 Markdown/路径/标记）+ 句子边界截断（≤60 字）。
/// 候选为 None / 系统噪声 / 清洗后无实质内容 → 「（无标题会话）」。
pub fn derive_title(candidate: Option<&str>) -> String {
    let cleaned = candidate
        .filter(|s| !is_system_noise(s))
        .and_then(clean_title_text);
    match cleaned {
        Some(s) => truncate_title(&s, 60),
        None => "（无标题会话）".to_string(),
    }
}

/// 在 ≤max 字范围内尽量于句末标点处截断；否则硬截 + 省略号。
/// 全程用字符索引（CJK 安全），不混用字节偏移。
fn truncate_title(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return s.to_string();
    }
    // 在 [0, max) 窗口内找最后一个句末标点的字符下标。
    let boundary = chars[..max]
        .iter()
        .rposition(|c| "。！？.!?\n".contains(*c));
    if let Some(pos) = boundary {
        // 含标点本身：取 [0, pos]。断点不太早（≥max/3）才采用，避免标题过短。
        if pos + 1 >= max / 3 {
            return chars[..=pos].iter().collect::<String>().trim_end().to_string();
        }
    }
    let head: String = chars[..max].iter().collect();
    format!("{}…", head.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn subagent_meta_prefers_description() {
        let dir = tempfile::tempdir().unwrap();
        let jsonl = dir.path().join("agent-x.jsonl");
        std::fs::File::create(&jsonl).unwrap().write_all(b"{}").unwrap();
        std::fs::File::create(dir.path().join("agent-x.meta.json"))
            .unwrap()
            .write_all(br#"{"agentType":"Explore","description":"Code search & business logic"}"#)
            .unwrap();
        let m = parse_subagent_meta(&jsonl).expect("should read meta");
        assert_eq!(m.name, "Code search & business logic");
        assert_eq!(m.agent_type, "Explore");
    }

    #[test]
    fn subagent_meta_falls_back_to_agent_type() {
        let dir = tempfile::tempdir().unwrap();
        let jsonl = dir.path().join("agent-y.jsonl");
        std::fs::File::create(&jsonl).unwrap().write_all(b"{}").unwrap();
        std::fs::File::create(dir.path().join("agent-y.meta.json"))
            .unwrap()
            .write_all(br#"{"agentType":"Explore"}"#)
            .unwrap();
        let m = parse_subagent_meta(&jsonl).unwrap();
        assert_eq!(m.name, "Explore", "无 description 时退回 agentType");
        assert_eq!(m.agent_type, "Explore");
    }

    #[test]
    fn subagent_meta_missing_file_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let jsonl = dir.path().join("agent-z.jsonl");
        std::fs::File::create(&jsonl).unwrap().write_all(b"{}").unwrap();
        // 无 .meta.json
        assert!(parse_subagent_meta(&jsonl).is_none());
    }

    #[test]
    fn flags_system_noise() {
        assert!(is_system_noise("<system-reminder>hi</system-reminder>"));
        assert!(is_system_noise("# AGENTS.md instructions"));
        assert!(is_system_noise("   "));
        assert!(is_system_noise("<command-name>/resume</command-name>"));
        assert!(is_system_noise("<skill><name>analysis</name>")); // 新增：<skill> 标签
        assert!(is_system_noise("<subagent_notification>{...}")); // 新增
        assert!(is_system_noise("Base directory for this skill: /Users/x")); // skill 注入文本
        assert!(!is_system_noise("帮我修复登录 bug"));
    }

    #[test]
    fn derives_clean_title() {
        assert_eq!(derive_title(Some("hi")), "hi");
        // 标题取首行（多行只留第一行有内容的）。
        assert_eq!(derive_title(Some("帮我  修复登录")), "帮我 修复登录");
        assert_eq!(derive_title(None), "（无标题会话）");
        assert_eq!(derive_title(Some("<system-reminder>x")), "（无标题会话）");
    }

    #[test]
    fn flags_greetings() {
        assert!(is_greeting("hi"));
        assert!(is_greeting("你好！"));
        assert!(is_greeting("Hello"));
        assert!(!is_greeting("帮我修复登录"));
    }

    #[test]
    fn truncates_long_title() {
        let long = "a".repeat(100);
        let t = derive_title(Some(&long));
        assert!(t.chars().count() <= 61);
        assert!(t.ends_with('…'));
    }

    // ---- 智能标题提炼新增 ----

    #[test]
    fn detects_command_invocation() {
        assert!(is_command_invocation("$analysis"));
        assert!(is_command_invocation("  $build something"));
        assert!(is_command_invocation("/resume"));
        assert!(is_command_invocation("请你使用 [$analysis](/Users/leocoder/.agents/skills/analysis/SKILL.md)"));
        assert!(is_command_invocation("[$analysis](/path)"));
        assert!(!is_command_invocation("帮我分析这段代码"));
        assert!(!is_command_invocation("$ 符号开头但非命令")); // $ 后非字母
    }

    #[test]
    fn command_invocation_not_used_as_title() {
        // 命令调用句 → 清洗返回 None → derive_title 兜底
        assert_eq!(derive_title(Some("$analysis")), "（无标题会话）");
        assert_eq!(
            derive_title(Some("请你使用 [$analysis](/Users/leocoder/.agents/skills/analysis/SKILL.md)")),
            "（无标题会话）"
        );
    }

    #[test]
    fn strips_markdown_link() {
        // [文字](url) → 文字
        assert_eq!(
            derive_title(Some("请分析 [源码模块](/path/to/SKILL.md) 的边界")),
            "请分析 源码模块 的边界"
        );
        // 纯链接（文字非命令）
        assert_eq!(derive_title(Some("[登录重构方案](/doc.md)")), "登录重构方案");
    }

    #[test]
    fn strips_inline_code_and_md_marks() {
        assert_eq!(derive_title(Some("修复 `parseUser` 的空指针")), "修复 parseUser 的空指针");
        assert_eq!(derive_title(Some("# 重构登录模块")), "重构登录模块");
        assert_eq!(derive_title(Some("> 引用的需求")), "引用的需求");
        assert_eq!(derive_title(Some("- 列表项任务")), "列表项任务");
    }

    #[test]
    fn pure_path_yields_no_title() {
        assert_eq!(derive_title(Some("/Users/leocoder/coderspace/develop/aistack")), "（无标题会话）");
        assert_eq!(derive_title(Some("~/projects/foo")), "（无标题会话）");
        // 路径 + 描述 → 保留（有空格、有信息）
        assert_eq!(
            derive_title(Some("在 /Users/leo/proj 下重构")),
            "在 /Users/leo/proj 下重构"
        );
    }

    #[test]
    fn clean_title_text_returns_none_for_junk() {
        assert!(clean_title_text("$analysis").is_none());
        assert!(clean_title_text("/Users/leo/x").is_none());
        assert!(clean_title_text("   ").is_none());
        assert!(clean_title_text("帮我看看登录 bug").is_some());
    }

    /// 不退化：现有好标题保持正确。
    #[test]
    fn good_titles_do_not_regress() {
        assert_eq!(derive_title(Some("你是谁")), "你是谁");
        assert_eq!(
            derive_title(Some("你负责只读分析当前仓库的后端架构与请求链路")),
            "你负责只读分析当前仓库的后端架构与请求链路"
        );
        assert_eq!(
            derive_title(Some("coi-ui 是原前端项目，coi-ui-acro 是我要完成的新前端模版项目")),
            "coi-ui 是原前端项目，coi-ui-acro 是我要完成的新前端模版项目"
        );
    }

    #[test]
    fn ai_fallback_when_user_side_empty() {
        use crate::models::{Message, Role};
        let msgs = vec![
            Message { role: Role::User, text: "$analysis".into(), ts: "t".into(), tool_name: None },
            Message {
                role: Role::Assistant,
                text: "我来分析登录模块的认证流程".into(),
                ts: "t".into(),
                tool_name: None,
            },
        ];
        // user 候选是命令句 → 提炼不出 → 用 AI 首条文本兜底。
        let title = title_with_ai_fallback(Some("$analysis"), &msgs);
        assert_eq!(title, "我来分析登录模块的认证流程");
    }

    #[test]
    fn ai_fallback_skips_tool_calls() {
        use crate::models::{Message, Role};
        let msgs = vec![
            Message { role: Role::Assistant, text: "ls -la".into(), ts: "t".into(), tool_name: Some("Bash".into()) },
            Message { role: Role::Assistant, text: "执行结果显示有 3 个文件".into(), ts: "t".into(), tool_name: None },
        ];
        let title = title_with_ai_fallback(None, &msgs);
        assert_eq!(title, "执行结果显示有 3 个文件", "应跳过工具调用，取首条 AI 文本");
    }

    #[test]
    fn user_side_wins_over_ai() {
        use crate::models::{Message, Role};
        let msgs = vec![Message {
            role: Role::Assistant,
            text: "AI 的回复".into(),
            ts: "t".into(),
            tool_name: None,
        }];
        // user 候选有效 → 不走 AI 兜底。
        let title = title_with_ai_fallback(Some("帮我重构登录模块"), &msgs);
        assert_eq!(title, "帮我重构登录模块");
    }

    #[test]
    fn truncates_at_sentence_boundary() {
        // 句号在 max=20 窗口内且不太早（位置 9，≥20/3）→ 在句号断句。
        let text = "前半句讲清核心需求。后半句还有很多补充说明文字继续展开论述细节内容";
        let t = truncate_title(text, 20);
        assert_eq!(t, "前半句讲清核心需求。", "应在句号处自然断句");

        // 窗口内无句末标点 → 硬截 + 省略号。
        let no_punct = "这是一段没有任何句末标点的连续中文文字内容一直延续下去没有停顿";
        let t2 = truncate_title(no_punct, 10);
        assert!(t2.ends_with('…'));
        assert_eq!(t2.chars().count(), 11); // 10 字 + 省略号
    }
}
