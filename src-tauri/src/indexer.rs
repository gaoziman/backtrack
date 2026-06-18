//! 扫描 → 并行解析 → 写入 Store。
use crate::models::{Message, Role, SessionMeta, Tool};
use crate::parsers::{claude, codex};
use crate::scanner::{scan_files, ScanItem};
use crate::store::Store;
use rayon::prelude::*;
use std::path::Path;

/// 标题提炼逻辑版本号。每次实质修改 `derive_title` 的提炼规则时 +1，
/// 触发下次启动全量重建以让旧会话标题用新逻辑重算。
pub const TITLE_LOGIC_VERSION: i64 = 1;

#[derive(serde::Serialize, Clone, Default)]
pub struct ScanSummary {
    pub total: usize,
    pub claude: usize,
    pub codex: usize,
}

/// 把消息拼成用于搜索的三路正文：(合并, 用户, AI)。
/// 合并供 LIKE 兜底与片段来源；用户/AI 供角色过滤。
///
/// **只索引对话正文（user / assistant 文本），排除工具调用与工具输出**：
/// 工具输出（命令结果、文件 dump、大段 JSON）体积巨大且全文搜索价值低，
/// 纳入 trigram 索引会使索引体积爆炸（实测 1500 会话曾达 2GB）。
/// 工具内容仍可在阅读器里查看（get_transcript 不受影响），只是不进搜索索引。
fn bodies_of(messages: &[Message]) -> (String, String, String) {
    let (mut all, mut user, mut ai) = (Vec::new(), Vec::new(), Vec::new());
    for m in messages {
        // 跳过工具调用 / 工具结果
        if m.tool_name.is_some() || matches!(m.role, Role::Tool) {
            continue;
        }
        all.push(m.text.as_str());
        match m.role {
            Role::User => user.push(m.text.as_str()),
            Role::Assistant => ai.push(m.text.as_str()),
            Role::Tool => {}
        }
    }
    (all.join("\n"), user.join("\n"), ai.join("\n"))
}

fn parse_item(item: &ScanItem) -> Option<(SessionMeta, Vec<Message>)> {
    match item.tool {
        Tool::Claude => claude::parse_claude(&item.path),
        Tool::Codex => codex::parse_codex(&item.path),
    }
}

/// 扫描两个根目录、**增量**同步到 store，返回当前索引统计。
///
/// 增量策略（治本启动慢）：
/// - 仅解析"新增"或"mtime 变化"的文件，未变文件跳过、保留既有索引行；
/// - 删除磁盘上已不存在的会话行；
/// - 不再全量 clear()，复用上次的持久化索引。
/// 首次（或加列迁移后）所有文件都算"新"，等价一次全量构建。
pub fn build_index(store: &Store, claude_root: &Path, codex_root: &Path) -> ScanSummary {
    let items = scan_files(claude_root, codex_root);
    let existing = store.indexed_mtimes().unwrap_or_default();

    // 标题逻辑升级检测：库内版本与当前代码版本不一致 → 本次忽略 mtime，全量重解析，
    // 让所有旧会话用新 derive_title 重算标题（用户自定义标题不受影响，读时 COALESCE 覆盖）。
    let force_full = store.title_logic_version().unwrap_or(0) != TITLE_LOGIC_VERSION;

    // 增量核心：仅解析新增或 mtime 变化的文件；force_full 时全部重解析。
    let parsed: Vec<(SessionMeta, Vec<Message>, i64)> = items
        .par_iter()
        .filter(|it| {
            force_full || existing.get(it.path.to_string_lossy().as_ref()) != Some(&it.mtime)
        })
        .filter_map(|it| parse_item(it).map(|(meta, msgs)| (meta, msgs, it.mtime)))
        .collect();

    for (meta, msgs, mtime) in &parsed {
        let (all, user, ai) = bodies_of(msgs);
        let _ = store.upsert(meta, &all, &user, &ai, *mtime);
    }

    // 删除磁盘上已消失的会话。
    let current: std::collections::HashSet<String> = items
        .iter()
        .map(|it| it.path.to_string_lossy().into_owned())
        .collect();
    let removed: Vec<String> = existing
        .keys()
        .filter(|p| !current.contains(*p))
        .cloned()
        .collect();
    if !removed.is_empty() {
        let _ = store.delete_paths(&removed);
    }

    // 全量重建后写入当前标题逻辑版本号，避免下次重复重建。
    if force_full {
        let _ = store.set_title_logic_version(TITLE_LOGIC_VERSION);
    }

    // 统计：当前磁盘上全部会话（present），按工具分。
    let mut summary = ScanSummary::default();
    for it in &items {
        match it.tool {
            Tool::Claude => summary.claude += 1,
            Tool::Codex => summary.codex += 1,
        }
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

    /// 构造一条最小可解析的 Claude 会话 jsonl（含一条 user 文本）。
    fn claude_jsonl(user_text: &str) -> String {
        format!(
            r#"{{"type":"user","cwd":"/Users/leo/ai","timestamp":"2026-06-16T01:00:00Z","message":{{"role":"user","content":"{}"}}}}"#,
            user_text
        )
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
        assert_eq!(store.search("旅迹", None, None).unwrap().len(), 1);
        assert_eq!(store.list_projects().unwrap().len(), 2);
    }

    /// 增量同步：未变跳过、变更重索引、删除移除、新增加入。
    #[test]
    fn incremental_sync_skips_unchanged_reindexes_changed_and_removes_deleted() {
        use std::time::{Duration, UNIX_EPOCH};
        let claude = tempfile::tempdir().unwrap();
        let codex = tempfile::tempdir().unwrap();
        let dir = claude.path().join("-Users-leo-ai");
        let file = dir.join("s1.jsonl");

        let set_mtime = |secs: u64| {
            let f = std::fs::File::options().write(true).open(&file).unwrap();
            f.set_modified(UNIX_EPOCH + Duration::from_secs(secs)).unwrap();
        };

        let store = Store::open_in_memory().unwrap();

        // 1. 首建：索引会话甲
        write(&dir, "s1.jsonl", &claude_jsonl("唯一标记甲"));
        set_mtime(1000);
        build_index(&store, claude.path(), codex.path());
        assert_eq!(store.search("唯一标记甲", None, None).unwrap().len(), 1);

        // 2. 改内容但 mtime 不变 → 跳过（索引仍是甲）
        write(&dir, "s1.jsonl", &claude_jsonl("唯一标记乙"));
        set_mtime(1000);
        build_index(&store, claude.path(), codex.path());
        assert_eq!(store.search("唯一标记乙", None, None).unwrap().len(), 0, "mtime 未变应跳过解析");
        assert_eq!(store.search("唯一标记甲", None, None).unwrap().len(), 1);

        // 3. mtime 变新 → 重新索引（变为乙）
        set_mtime(2000);
        build_index(&store, claude.path(), codex.path());
        assert_eq!(store.search("唯一标记乙", None, None).unwrap().len(), 1, "mtime 变化应重索引");
        assert_eq!(store.search("唯一标记甲", None, None).unwrap().len(), 0);

        // 4. 删除文件 → 索引移除
        std::fs::remove_file(&file).unwrap();
        build_index(&store, claude.path(), codex.path());
        assert_eq!(store.count().unwrap(), 0, "文件删除后应从索引移除");

        // 5. 新增文件 → 索引新增
        write(&dir, "s2.jsonl", &claude_jsonl("全新会话丙"));
        build_index(&store, claude.path(), codex.path());
        assert_eq!(store.search("全新会话丙", None, None).unwrap().len(), 1);
    }

    /// 标题逻辑版本号驱动全量重建：版本不匹配时，即使 mtime 未变也重解析。
    #[test]
    fn title_version_mismatch_forces_full_rebuild() {
        use std::time::{Duration, UNIX_EPOCH};
        let claude = tempfile::tempdir().unwrap();
        let codex = tempfile::tempdir().unwrap();
        let dir = claude.path().join("-Users-leo-ai");
        let file = dir.join("s1.jsonl");
        let store = Store::open_in_memory().unwrap();

        // 首建：写入会话甲，mtime 固定。
        write(&dir, "s1.jsonl", &claude_jsonl("标题甲内容"));
        let f = std::fs::File::options().write(true).open(&file).unwrap();
        f.set_modified(UNIX_EPOCH + Duration::from_secs(1000)).unwrap();
        drop(f);
        build_index(&store, claude.path(), codex.path());
        assert_eq!(store.title_logic_version().unwrap(), TITLE_LOGIC_VERSION, "首建应写入版本号");

        // 改内容、mtime 不变：版本匹配 → 增量跳过（标题不变）。
        write(&dir, "s1.jsonl", &claude_jsonl("标题乙内容"));
        let f = std::fs::File::options().write(true).open(&file).unwrap();
        f.set_modified(UNIX_EPOCH + Duration::from_secs(1000)).unwrap();
        drop(f);
        build_index(&store, claude.path(), codex.path());
        assert_eq!(store.search("标题乙内容", None, None).unwrap().len(), 0, "版本匹配+mtime未变应跳过");

        // 模拟"derive_title 升级"：把库内版本号改旧 → 下次 build 强制全量重建。
        store.set_title_logic_version(TITLE_LOGIC_VERSION - 1).unwrap();
        build_index(&store, claude.path(), codex.path());
        assert_eq!(
            store.search("标题乙内容", None, None).unwrap().len(),
            1,
            "版本不匹配应忽略 mtime 全量重解析，标题乙生效"
        );
        assert_eq!(store.title_logic_version().unwrap(), TITLE_LOGIC_VERSION, "重建后版本号回正");
    }

    /// 真实数据增量计时（默认忽略）：
    /// `cargo test real_data_incremental_timing -- --ignored --nocapture`
    /// 验证"首建全量慢、复建增量近乎瞬时"。
    #[test]
    #[ignore]
    fn real_data_incremental_timing() {
        use crate::scanner::{default_claude_root, default_codex_root};
        use std::time::Instant;
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(&tmp.path().join("index.db")).unwrap();
        let c = default_claude_root().unwrap();
        let x = default_codex_root().unwrap();

        let t1 = Instant::now();
        let s1 = build_index(&store, &c, &x);
        let d1 = t1.elapsed();

        let t2 = Instant::now();
        let s2 = build_index(&store, &c, &x);
        let d2 = t2.elapsed();

        println!("\n== 增量索引计时 ==");
        println!("首建(全量): {} 会话, 耗时 {:?}", s1.total, d1);
        println!("复建(增量,无变化): {} 会话, 耗时 {:?}", s2.total, d2);
        assert_eq!(s1.total, s2.total, "两次会话总数应一致");
        assert!(d2 < d1, "增量复建应快于全量首建");
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
        let hits = store.search("旅迹", None, None).unwrap();
        println!("搜索「旅迹」命中 {} 个:", hits.len());
        for h in hits.iter().take(5) {
            println!("  [{}] {} — {}", h.meta.tool.as_str(), h.meta.title, h.meta.id);
        }
        assert!(sum.total > 0);
    }

    /// 真实数据标题质量诊断（默认忽略）：统计唯一标题数 / 无标题数 / Top 撞车。
    /// cargo test real_title_uniqueness -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_title_uniqueness() {
        use crate::scanner::{default_claude_root, default_codex_root};
        use std::collections::HashSet;
        let store = Store::open_in_memory().unwrap();
        let c = default_claude_root().unwrap();
        let x = default_codex_root().unwrap();
        build_index(&store, &c, &x);
        // 取所有会话标题统计唯一数
        let mut titles = Vec::new();
        for p in store.list_projects().unwrap() {
            for s in store.list_sessions(&p.path).unwrap() {
                titles.push(s.title);
            }
        }
        let total = titles.len();
        let uniq: HashSet<_> = titles.iter().collect();
        let untitled = titles.iter().filter(|t| t.as_str()=="（无标题会话）").count();
        println!("\n== 升级后标题统计 ==");
        println!("总会话: {}", total);
        println!("唯一标题数: {}", uniq.len());
        println!("无标题会话数: {}", untitled);
        // Top 撞车
        use std::collections::HashMap;
        let mut cnt: HashMap<&str,usize> = HashMap::new();
        for t in &titles { *cnt.entry(t).or_insert(0)+=1; }
        let mut v: Vec<_> = cnt.iter().collect();
        v.sort_by(|a,b| b.1.cmp(a.1));
        println!("Top 5 撞车标题:");
        for (t,c) in v.iter().take(5) {
            let short: String = t.chars().take(30).collect();
            println!("  {:3}次  {}", c, short);
        }
    }
}
