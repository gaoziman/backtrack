//! 单会话导出为 Markdown / HTML（纯渲染函数，可单测；不做 IO）。
//! HTML 用 comrak 渲染 Markdown（GFM）+ syntect 高亮代码块，自包含离线、防 XSS。
use crate::models::{Message, Role, SessionMeta};
use comrak::plugins::syntect::SyntectAdapter;
use comrak::{markdown_to_html_with_plugins, Options, Plugins};

/// syntect 内置主题（暖调深色，匹配「Claude 暖」方案的代码块底色，高对比可读）。
const CODE_THEME: &str = "base16-eighties.dark";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExportFormat {
    Markdown,
    Html,
}

impl ExportFormat {
    pub fn from_str(s: &str) -> Option<ExportFormat> {
        match s {
            "md" | "markdown" => Some(ExportFormat::Markdown),
            "html" => Some(ExportFormat::Html),
            _ => None,
        }
    }
    pub fn ext(&self) -> &'static str {
        match self {
            ExportFormat::Markdown => "md",
            ExportFormat::Html => "html",
        }
    }
}

/// 渲染会话为目标格式文本。
pub fn render(
    meta: &SessionMeta,
    messages: &[Message],
    format: ExportFormat,
    include_tools: bool,
) -> String {
    match format {
        ExportFormat::Markdown => render_markdown(meta, messages, include_tools),
        ExportFormat::Html => render_html(meta, messages, include_tools),
    }
}

/// 是否应跳过该消息（工具调用 / 工具输出，在 include_tools=false 时跳过）。
fn is_tool_message(m: &Message) -> bool {
    m.tool_name.is_some() || matches!(m.role, Role::Tool)
}

/// 渲染为 Markdown。
pub fn render_markdown(meta: &SessionMeta, messages: &[Message], include_tools: bool) -> String {
    let title = if meta.title.trim().is_empty() {
        "未命名会话"
    } else {
        meta.title.trim()
    };
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", title));
    out.push_str(&format!(
        "> 工具: {} · 目录: {} · 开始: {} · 消息数: {}\n\n---\n\n",
        meta.tool.as_str(),
        meta.cwd,
        meta.started_at,
        meta.message_count
    ));

    for m in messages {
        if !include_tools && is_tool_message(m) {
            continue;
        }
        match (m.role, &m.tool_name) {
            (_, Some(name)) => {
                out.push_str(&format!("## 🔧 工具调用：{}\n\n```\n{}\n```\n\n", name, m.text));
            }
            (Role::Tool, _) => {
                out.push_str(&format!("## 📤 工具输出\n\n```\n{}\n```\n\n", m.text));
            }
            (Role::User, _) => {
                out.push_str(&format!("## 👤 用户\n\n{}\n\n", m.text));
            }
            (Role::Assistant, _) => {
                out.push_str(&format!("## 🤖 助手\n\n{}\n\n", m.text));
            }
        }
    }
    out
}

/// HTML 文本转义（用于标题/元信息等非 Markdown 文本）。
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// comrak 选项：开启常用 GFM 扩展，关闭裸 HTML（unsafe_=false，转义防 XSS）。
fn comrak_options() -> Options<'static> {
    let mut o = Options::default();
    o.extension.strikethrough = true;
    o.extension.table = true;
    o.extension.autolink = true;
    o.extension.tasklist = true;
    o.render.unsafe_ = false; // 转义正文里的裸 HTML，防注入
    o
}

/// 把一段 Markdown 正文渲染成 HTML（代码块经 syntect 高亮）。
fn md_to_html(text: &str) -> String {
    let opts = comrak_options();
    let adapter = SyntectAdapter::new(Some(CODE_THEME));
    let mut plugins = Plugins::default();
    plugins.render.codefence_syntax_highlighter = Some(&adapter);
    markdown_to_html_with_plugins(text, &opts, &plugins)
}

/// 把工具调用/输出渲染成高亮代码块（包成 ```lang 交给 syntect）。
fn tool_to_html(text: &str, lang: &str) -> String {
    let fenced = format!("```{}\n{}\n```", lang, text);
    md_to_html(&fenced)
}

/// 渲染为自包含 HTML（方案 A · Claude 暖；内联样式，无外部资源，离线可看）。
pub fn render_html(meta: &SessionMeta, messages: &[Message], include_tools: bool) -> String {
    let title = if meta.title.trim().is_empty() {
        "未命名会话"
    } else {
        meta.title.trim()
    };

    let mut body = String::new();
    for m in messages {
        if !include_tools && is_tool_message(m) {
            continue;
        }
        let (role_cls, label, tag) = match (m.role, &m.tool_name) {
            (_, Some(name)) => ("ai", format!("助手 · 工具调用：{}", name), "Tool"),
            (Role::Tool, _) => ("ai", "助手 · 工具输出".to_string(), "Tool"),
            (Role::User, _) => ("user", "用户".to_string(), "User"),
            (Role::Assistant, _) => ("ai", "助手".to_string(), "Assistant"),
        };
        // 工具调用/输出 → 高亮代码块；普通对话 → Markdown 渲染。
        let inner = if m.tool_name.is_some() {
            tool_to_html(&m.text, "bash")
        } else if matches!(m.role, Role::Tool) {
            tool_to_html(&m.text, "")
        } else {
            md_to_html(&m.text)
        };
        body.push_str(&format!(
            "<div class=\"msg {role}\">\n\
               <div class=\"role\"><span class=\"dot\"></span>{label} <span class=\"tag\">{tag}</span></div>\n\
               <div class=\"bubble\">{inner}</div>\n\
             </div>\n",
            role = role_cls,
            label = html_escape(&label),
            tag = tag,
            inner = inner,
        ));
    }

    format!(
        "<!DOCTYPE html>\n<html lang=\"zh\">\n<head>\n<meta charset=\"utf-8\">\n\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
<title>{title}</title>\n<style>\n{css}</style>\n</head>\n<body>\n\
<div class=\"page\">\n\
<header class=\"doc-head\">\n\
<div class=\"doc-tool\"><span class=\"tdot\"></span> {tool} 会话</div>\n\
<h1 class=\"doc-title\">{title}</h1>\n\
<div class=\"doc-meta\"><span>目录 <b>{cwd}</b></span><span>开始 <b>{started}</b></span><span><b>{count}</b> 条消息</span></div>\n\
</header>\n\
{body}\
<footer class=\"doc-foot\">由 Backtrack 导出 · 纯本地 · 内容与原始会话一致</footer>\n\
</div>\n</body>\n</html>\n",
        title = html_escape(title),
        css = CLAUDE_WARM_CSS,
        tool = html_escape(meta.tool.as_str()),
        cwd = html_escape(&meta.cwd),
        started = html_escape(&meta.started_at),
        count = meta.message_count,
        body = body,
    )
}

/// 方案 A · Claude 暖 的完整内联 CSS（与确认的预览 1:1）。
/// 代码块底色与高亮由 syntect 内联输出，这里只管文档骨架与气泡。
const CLAUDE_WARM_CSS: &str = r#"
:root{
  --canvas:#faf9f5; --surface-soft:#f5f0e8;
  --ink:#141413; --body:#3d3d3a; --muted:#6c6a64; --hairline:#e6dfd8;
  --accent:#cc785c;
  --user-bg:#f3ede2; --ai-bg:#ffffff;
  --user-dot:#cc785c; --ai-dot:#a8a49a;
  --display-font:"Tiempos Headline",Georgia,"Songti SC",serif;
  --sans:"Inter",-apple-system,BlinkMacSystemFont,"PingFang SC",sans-serif;
  --mono:"SF Mono",ui-monospace,"JetBrains Mono",Menlo,monospace;
}
*{box-sizing:border-box;}
html{-webkit-font-smoothing:antialiased;}
body{margin:0;background:var(--canvas);color:var(--body);font-family:var(--sans);font-size:15.5px;line-height:1.7;}
.page{max-width:920px;margin:0 auto;padding:56px 24px 96px;}
.doc-head{margin-bottom:40px;padding-bottom:24px;border-bottom:1px solid var(--hairline);}
.doc-tool{display:inline-flex;align-items:center;gap:6px;font-size:12px;font-weight:600;letter-spacing:.04em;color:var(--accent);margin-bottom:14px;}
.doc-tool .tdot{width:7px;height:7px;border-radius:50%;background:var(--accent);}
h1.doc-title{font-family:var(--display-font);font-weight:500;font-size:32px;line-height:1.2;letter-spacing:-0.4px;color:var(--ink);margin:0 0 16px;}
.doc-meta{display:flex;flex-wrap:wrap;gap:6px 14px;font-size:13px;color:var(--muted);}
.doc-meta b{color:var(--body);font-weight:600;}
.msg{margin:28px 0;}
.msg .role{display:flex;align-items:center;gap:8px;font-size:13px;font-weight:600;margin-bottom:10px;}
.msg .role .dot{width:8px;height:8px;border-radius:50%;flex:none;}
.msg.user .role{color:var(--ink);}
.msg.user .role .dot{background:var(--user-dot);}
.msg.ai .role{color:var(--muted);}
.msg.ai .role .dot{background:var(--ai-dot);}
.role .tag{font-size:10.5px;font-weight:500;color:var(--muted);border:1px solid var(--hairline);border-radius:5px;padding:1px 6px;}
.bubble{background:var(--ai-bg);border:1px solid var(--hairline);border-radius:7px;padding:4px 22px;}
.msg.user .bubble{background:var(--user-bg);border-color:transparent;}
.bubble>*:first-child{margin-top:14px;}
.bubble>*:last-child{margin-bottom:14px;}
.bubble h1,.bubble h2{font-family:var(--display-font);font-weight:600;color:var(--ink);letter-spacing:-0.2px;}
.bubble h1{font-size:22px;margin:22px 0 12px;}
.bubble h2{font-size:20px;margin:20px 0 12px;}
.bubble h3{font-size:16.5px;font-weight:650;color:var(--ink);margin:18px 0 10px;}
.bubble p{margin:12px 0;}
.bubble strong{color:var(--ink);font-weight:650;}
.bubble ul,.bubble ol{margin:12px 0;padding-left:22px;}
.bubble li{margin:5px 0;}
.bubble a{color:var(--accent);text-decoration:none;border-bottom:1px solid color-mix(in srgb,var(--accent) 35%,transparent);}
.bubble blockquote{margin:14px 0;padding:2px 0 2px 16px;border-left:3px solid var(--accent);color:var(--muted);}
.bubble table{border-collapse:collapse;margin:14px 0;width:100%;font-size:14px;}
.bubble th,.bubble td{border:1px solid var(--hairline);padding:7px 11px;text-align:left;}
.bubble th{background:var(--surface-soft);color:var(--ink);font-weight:600;}
.bubble :not(pre)>code{font-family:var(--mono);font-size:0.88em;background:var(--surface-soft);color:var(--ink);padding:2px 6px;border-radius:5px;border:1px solid var(--hairline);}
.bubble pre{border-radius:10px;padding:16px 18px;overflow-x:auto;margin:14px 0;font-size:13.5px;line-height:1.7;}
.bubble pre code{font-family:var(--mono);background:none;border:none;padding:0;}
.doc-foot{margin-top:56px;padding-top:20px;border-top:1px solid var(--hairline);font-size:12px;color:var(--muted);text-align:center;}
"#;

/// 把会话标题净化为安全文件名（去文件系统非法字符，截断 80 字符，空则 fallback）。
pub fn safe_file_name(title: &str, ext: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    let base: String = trimmed.chars().take(80).collect();
    let base = base.trim();
    let stem = if base.is_empty() { "未命名会话" } else { base };
    format!("{}.{}", stem, ext)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Message, Role, SessionMeta, Tool};

    fn meta() -> SessionMeta {
        SessionMeta {
            id: "abc".into(),
            tool: Tool::Claude,
            cwd: "/Users/leo/proj".into(),
            file_path: "/p/abc.jsonl".into(),
            title: "修复登录 bug".into(),
            started_at: "2026-06-17T01:00:00Z".into(),
            updated_at: "2026-06-17T02:00:00Z".into(),
            message_count: 4,
            forked_from: None,
            resume_command: "claude --resume abc".into(),
            has_children: false,
        }
    }

    fn msg(role: Role, text: &str, tool_name: Option<&str>) -> Message {
        Message {
            role,
            text: text.into(),
            ts: "2026-06-17T01:00:00Z".into(),
            tool_name: tool_name.map(|s| s.to_string()),
        }
    }

    fn sample() -> Vec<Message> {
        vec![
            msg(Role::User, "帮我修复登录", None),
            msg(Role::Assistant, "好的，我来看看", None),
            msg(Role::Assistant, "ls -la", Some("Bash")),
            msg(Role::Tool, "total 8\nfile.rs", None),
        ]
    }

    #[test]
    fn markdown_has_title_and_meta() {
        let md = render_markdown(&meta(), &sample(), true);
        assert!(md.starts_with("# 修复登录 bug"));
        assert!(md.contains("工具: claude"));
        assert!(md.contains("目录: /Users/leo/proj"));
    }

    #[test]
    fn markdown_includes_user_and_assistant() {
        let md = render_markdown(&meta(), &sample(), true);
        assert!(md.contains("👤 用户"));
        assert!(md.contains("帮我修复登录"));
        assert!(md.contains("🤖 助手"));
        assert!(md.contains("好的，我来看看"));
    }

    #[test]
    fn markdown_includes_tools_when_flag_true() {
        let md = render_markdown(&meta(), &sample(), true);
        assert!(md.contains("🔧 工具调用：Bash"));
        assert!(md.contains("ls -la"));
        assert!(md.contains("📤 工具输出"));
        assert!(md.contains("file.rs"));
    }

    #[test]
    fn markdown_excludes_tools_when_flag_false() {
        let md = render_markdown(&meta(), &sample(), false);
        assert!(!md.contains("🔧 工具调用"));
        assert!(!md.contains("📤 工具输出"));
        assert!(!md.contains("ls -la"));
        // 普通对话仍在
        assert!(md.contains("帮我修复登录"));
        assert!(md.contains("好的，我来看看"));
    }

    #[test]
    fn html_is_self_contained_and_escaped() {
        let mut msgs = sample();
        msgs.push(msg(Role::User, "试试 <script>alert(1)</script>", None));
        let html = render_html(&meta(), &msgs, true);
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<style>"));
        // 无外部资源引用（自包含、离线可看）
        assert!(!html.contains("src=\"http"));
        assert!(!html.contains("href=\"http"));
        assert!(!html.contains("@import"));
        assert!(!html.contains("cdn"));
        // XSS 防护：comrak unsafe_=false 会移除裸 HTML 标签（替换为注释），
        // 裸 <script> 绝不原样出现，无执行可能。
        assert!(!html.contains("<script>alert(1)</script>"));
        assert!(!html.contains("<script>"));
    }

    #[test]
    fn html_renders_markdown_structure() {
        // 普通对话里的 Markdown 应被真正渲染成 HTML 结构（而非原样文本）。
        let msgs = vec![
            msg(Role::User, "看下面", None),
            msg(
                Role::Assistant,
                "## 标题\n\n- 列表项\n\n```bash\ncurl -s https://x.cf\n```\n\n> 引用",
                None,
            ),
        ];
        let html = render_html(&meta(), &msgs, true);
        // 标题渲染为 <h2>，不再是 "## 标题"
        assert!(html.contains("<h2>") && html.contains("标题"));
        assert!(!html.contains("## 标题"));
        // 列表渲染为 <li>
        assert!(html.contains("<li>") && html.contains("列表项"));
        // 引用渲染为 <blockquote>
        assert!(html.contains("<blockquote>"));
        // 代码块渲染为 <pre>（syntect 高亮）
        assert!(html.contains("<pre"));
        assert!(html.contains("curl"));
    }

    #[test]
    fn html_excludes_tools_when_flag_false() {
        let html = render_html(&meta(), &sample(), false);
        assert!(!html.contains("工具调用"));
        assert!(!html.contains("工具输出"));
    }

    #[test]
    fn html_includes_tools_when_flag_true() {
        let html = render_html(&meta(), &sample(), true);
        assert!(html.contains("工具调用：Bash"));
        assert!(html.contains("工具输出"));
    }

    #[test]
    fn safe_file_name_sanitizes_illegal_chars() {
        assert_eq!(safe_file_name("a/b:c*d?", "md"), "a_b_c_d_.md");
        assert_eq!(safe_file_name("正常标题", "html"), "正常标题.html");
        assert_eq!(safe_file_name("   ", "md"), "未命名会话.md");
        assert_eq!(safe_file_name("", "md"), "未命名会话.md");
    }

    #[test]
    fn safe_file_name_truncates_long_title() {
        let long = "字".repeat(200);
        let name = safe_file_name(&long, "md");
        // 80 字 + ".md"
        assert!(name.chars().filter(|&c| c == '字').count() == 80);
        assert!(name.ends_with(".md"));
    }

    #[test]
    fn empty_title_falls_back_in_render() {
        let mut m = meta();
        m.title = "   ".into();
        let md = render_markdown(&m, &sample(), true);
        assert!(md.starts_with("# 未命名会话"));
    }
}
