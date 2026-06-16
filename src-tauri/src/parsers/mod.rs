//! 两种 jsonl 格式的解析器 + 公共辅助。
pub mod claude;
pub mod codex;

use serde_json::Value;

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
    t.starts_with('<')
        || t.starts_with("# AGENTS")
        || t.starts_with("Caveat")
        || t.starts_with("/resume")
        || t.starts_with("<command-")
        || t.starts_with("<permissions")
        || head.contains("system-reminder")
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

/// 从首条有效用户文本生成简洁标题（单行，≤60 字符）。
pub fn derive_title(first_user_text: Option<&str>) -> String {
    match first_user_text {
        Some(s) if !is_system_noise(s) => {
            let cleaned: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
            let truncated: String = cleaned.chars().take(60).collect();
            if truncated.chars().count() < cleaned.chars().count() {
                format!("{}…", truncated.trim_end())
            } else {
                truncated
            }
        }
        _ => "（无标题会话）".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_system_noise() {
        assert!(is_system_noise("<system-reminder>hi</system-reminder>"));
        assert!(is_system_noise("# AGENTS.md instructions"));
        assert!(is_system_noise("   "));
        assert!(is_system_noise("<command-name>/resume</command-name>"));
        assert!(!is_system_noise("帮我修复登录 bug"));
    }

    #[test]
    fn derives_clean_title() {
        assert_eq!(derive_title(Some("hi")), "hi");
        assert_eq!(derive_title(Some("帮我  修复\n登录")), "帮我 修复 登录");
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
}
