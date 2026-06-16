//! 扫描 → 并行解析 → 写入 Store。
use crate::models::{Message, SessionMeta, Tool};
use crate::parsers::{claude, codex};
use crate::scanner::{scan_files, ScanItem};
use crate::store::Store;
use rayon::prelude::*;
use std::path::Path;

#[derive(serde::Serialize, Clone, Default)]
pub struct ScanSummary {
    pub total: usize,
    pub claude: usize,
    pub codex: usize,
}

/// 把消息拼成用于搜索的正文。
fn body_of(messages: &[Message]) -> String {
    messages
        .iter()
        .map(|m| m.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_item(item: &ScanItem) -> Option<(SessionMeta, Vec<Message>)> {
    match item.tool {
        Tool::Claude => claude::parse_claude(&item.path),
        Tool::Codex => codex::parse_codex(&item.path),
    }
}

/// 扫描两个根目录、解析全部会话、写入 store。返回统计。
pub fn build_index(store: &Store, claude_root: &Path, codex_root: &Path) -> ScanSummary {
    let items = scan_files(claude_root, codex_root);

    // 并行解析（IO + JSON 解析是瓶颈）。
    let parsed: Vec<(SessionMeta, Vec<Message>)> =
        items.par_iter().filter_map(parse_item).collect();

    let mut summary = ScanSummary::default();
    let _ = store.clear();
    for (meta, msgs) in &parsed {
        match meta.tool {
            Tool::Claude => summary.claude += 1,
            Tool::Codex => summary.codex += 1,
        }
        let _ = store.upsert(meta, &body_of(msgs));
    }
    summary.total = summary.claude + summary.codex;
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write(dir: &Path, name: &str, content: &str) {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::File::create(&p).unwrap().write_all(content.as_bytes()).unwrap();
    }

    #[test]
    fn builds_index_from_both_tools() {
        let claude = tempfile::tempdir().unwrap();
        let codex = tempfile::tempdir().unwrap();

        write(
            &claude.path().join("-Users-leo-ai"),
            "s1.jsonl",
            r#"{"type":"user","cwd":"/Users/leo/ai","timestamp":"2026-06-16T01:00:00Z","message":{"role":"user","content":"旅迹原型"}}
{"type":"assistant","timestamp":"2026-06-16T01:00:05Z","message":{"role":"assistant","content":[{"type":"text","text":"好的"}]}}"#,
        );
        write(
            &codex.path().join("2026/03/16"),
            "rollout-x-uuid1.jsonl",
            r#"{"timestamp":"2026-03-16T03:00:00Z","type":"session_meta","payload":{"id":"uuid1","cwd":"/Users/leo/hub"}}
{"timestamp":"2026-03-16T03:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"重构布局"}]}}"#,
        );

        let store = Store::open_in_memory().unwrap();
        let summary = build_index(&store, claude.path(), codex.path());

        assert_eq!(summary.total, 2);
        assert_eq!(summary.claude, 1);
        assert_eq!(summary.codex, 1);
        assert_eq!(store.count().unwrap(), 2);
        assert_eq!(store.search("旅迹").unwrap().len(), 1);
        assert_eq!(store.list_projects().unwrap().len(), 2);
    }

    /// 针对真实磁盘数据的冒烟测试（默认忽略）：
    /// `cargo test real_data_smoke -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_data_smoke() {
        use crate::scanner::{default_claude_root, default_codex_root};
        let store = Store::open_in_memory().unwrap();
        let c = default_claude_root().unwrap();
        let x = default_codex_root().unwrap();
        let sum = build_index(&store, &c, &x);
        println!("\n== 真实数据索引 ==");
        println!("总计 {} (claude {} / codex {})", sum.total, sum.claude, sum.codex);
        let projs = store.list_projects().unwrap();
        println!("项目目录 {} 个，前 5:", projs.len());
        for p in projs.iter().take(5) {
            println!("  {} ({})", p.display_name, p.session_count);
        }
        let hits = store.search("旅迹").unwrap();
        println!("搜索「旅迹」命中 {} 个:", hits.len());
        for h in hits.iter().take(5) {
            println!("  [{}] {} — {}", h.tool.as_str(), h.title, h.id);
        }
        assert!(sum.total > 0);
    }
}
