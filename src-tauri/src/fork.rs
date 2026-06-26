//! Fork 谱系树构建：由「同 cwd 全部会话」构造以链顶为根的 fork 树。
//!
//! `forked_from` 存父会话的 **id**（非 file_path）。本模块在内存中把扁平会话列表
//! 连成树：从当前会话沿 `forked_from` 上溯找链顶，再 DFS 向下展开所有后代分支。
//! 纯函数（不触 DB/IO），便于单测；缺父占位 + visited 防环保证健壮。
use crate::models::SessionMeta;
use std::collections::{HashMap, HashSet};

/// fork 树节点：会话元数据 + 子节点 + 是否为「缺父占位」。
/// `meta` 为 `None` 表示占位节点（父不在本地索引），前端按 `missing` 区分渲染。
#[derive(serde::Serialize, Clone, Debug)]
pub struct ForkNode {
    #[serde(flatten)]
    pub meta: Option<SessionMeta>,
    /// true = 占位节点（forked_from 指向的父不在本地）。
    pub missing: bool,
    /// 是否为发起查询的当前会话（前端高亮 + 滚动定位）。
    pub is_current: bool,
    pub children: Vec<ForkNode>,
}

/// 由「同 cwd 全部会话」+「当前会话 file_path」构建 fork 树（链顶为根）。
///
/// - 找当前会话所属 fork 链的链顶（沿 forked_from 上溯到父为 None 或父不在集合）；
/// - 若链顶因父缺失而中断，则在其上挂一个 `missing` 占位父节点；
/// - 从链顶 DFS 构树，子节点按 started_at 升序（fork 发生先后）；
/// - `is_current` 标记当前会话；`visited` 防自引用/循环。
///
/// 当前会话不在 `sessions` 中（理论不应发生）时，返回一个仅含占位的退化树。
pub fn build_fork_tree(sessions: &[SessionMeta], current_file_path: &str) -> ForkNode {
    // id -> meta
    let by_id: HashMap<&str, &SessionMeta> =
        sessions.iter().map(|s| (s.id.as_str(), s)).collect();
    // file_path -> meta（定位当前会话）
    let current = sessions.iter().find(|s| s.file_path == current_file_path);

    // parent id -> children metas（仅当父存在于集合内）
    let mut children_of: HashMap<&str, Vec<&SessionMeta>> = HashMap::new();
    for s in sessions {
        if let Some(pid) = s.forked_from.as_deref() {
            if by_id.contains_key(pid) {
                children_of.entry(pid).or_default().push(s);
            }
        }
    }
    // 子节点按时间升序排列（稳定、贴合 fork 先后）。
    for v in children_of.values_mut() {
        v.sort_by(|a, b| a.started_at.cmp(&b.started_at).then(a.id.cmp(&b.id)));
    }

    let current = match current {
        Some(c) => c,
        // 当前会话不在集合：退化为占位根（健壮兜底，不 panic）。
        None => {
            return ForkNode { meta: None, missing: true, is_current: false, children: vec![] }
        }
    };

    // 沿 forked_from 上溯找链顶。记录链顶是否因父缺失而中断。
    let mut top = current;
    let mut up_visited: HashSet<&str> = HashSet::new();
    up_visited.insert(top.id.as_str());
    let mut missing_parent: Option<&str> = None;
    loop {
        match top.forked_from.as_deref() {
            None => break, // 真正的根
            Some(pid) => match by_id.get(pid) {
                Some(parent) => {
                    if !up_visited.insert(parent.id.as_str()) {
                        break; // 环：停在此处
                    }
                    top = parent;
                }
                None => {
                    missing_parent = Some(pid); // 父不在本地 → 链顶之上挂占位
                    break;
                }
            },
        }
    }

    // 从链顶 DFS 构建子树。
    let mut down_visited: HashSet<&str> = HashSet::new();
    let root_subtree = build_subtree(top, &children_of, current.id.as_str(), &mut down_visited);

    // 若链顶有缺失的父，包一层占位父节点。
    if missing_parent.is_some() {
        ForkNode {
            meta: None,
            missing: true,
            is_current: false,
            children: vec![root_subtree],
        }
    } else {
        root_subtree
    }
}

/// 递归构建以 `node` 为根的子树（DFS，visited 防环）。
fn build_subtree<'a>(
    node: &'a SessionMeta,
    children_of: &HashMap<&'a str, Vec<&'a SessionMeta>>,
    current_id: &str,
    visited: &mut HashSet<&'a str>,
) -> ForkNode {
    let children = if visited.insert(node.id.as_str()) {
        children_of
            .get(node.id.as_str())
            .map(|kids| {
                kids.iter()
                    .map(|c| build_subtree(c, children_of, current_id, visited))
                    .collect()
            })
            .unwrap_or_default()
    } else {
        // 已访问（环）→ 不再展开子节点。
        vec![]
    };

    ForkNode {
        meta: Some(node.clone()),
        missing: false,
        is_current: node.id == current_id,
        children,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Tool;

    /// 构造测试会话。forked_from 为父 id；file_path 形如 /f/<id>。
    fn s(id: &str, parent: Option<&str>, started: &str) -> SessionMeta {
        SessionMeta {
            id: id.into(),
            tool: Tool::Codex,
            cwd: "/p/ai".into(),
            file_path: format!("/f/{}", id),
            title: format!("会话 {}", id),
            started_at: started.into(),
            updated_at: started.into(),
            message_count: 1,
            forked_from: parent.map(|x| x.into()),
            resume_command: format!("codex resume '{}'", id),
            has_children: false,
            favorited: false,
            collection_ids: Vec::new(),
            parent_id: None,
            subagent_count: 0,
        }
    }

    /// 收集树中所有出现的（非占位）节点 id，便于断言。
    fn ids(node: &ForkNode, out: &mut Vec<String>) {
        if let Some(m) = &node.meta {
            out.push(m.id.clone());
        }
        for c in &node.children {
            ids(c, out);
        }
    }

    /// 查找树中标记为 current 的节点 id。
    fn current_id(node: &ForkNode) -> Option<String> {
        if node.is_current {
            return node.meta.as_ref().map(|m| m.id.clone());
        }
        node.children.iter().find_map(current_id)
    }

    /// 孤立会话（无父无子）→ 单节点树，根即自身且 is_current。
    #[test]
    fn isolated_session_is_single_node() {
        let sessions = vec![s("a", None, "2026-01-01")];
        let tree = build_fork_tree(&sessions, "/f/a");
        assert_eq!(tree.meta.as_ref().unwrap().id, "a");
        assert!(tree.is_current);
        assert!(!tree.missing);
        assert!(tree.children.is_empty());
    }

    /// 单父单子：从子发起，链顶为父，子标记 current。
    #[test]
    fn parent_child_chain_roots_at_top() {
        let sessions = vec![
            s("parent", None, "2026-01-01"),
            s("child", Some("parent"), "2026-01-02"),
        ];
        let tree = build_fork_tree(&sessions, "/f/child");
        assert_eq!(tree.meta.as_ref().unwrap().id, "parent"); // 根=链顶
        assert_eq!(tree.children.len(), 1);
        assert_eq!(tree.children[0].meta.as_ref().unwrap().id, "child");
        assert_eq!(current_id(&tree).as_deref(), Some("child"));
    }

    /// 一父多子：全部子并列，按 started_at 升序。
    #[test]
    fn parent_with_many_children_sorted_by_time() {
        let sessions = vec![
            s("p", None, "2026-01-01"),
            s("c3", Some("p"), "2026-01-04"),
            s("c1", Some("p"), "2026-01-02"),
            s("c2", Some("p"), "2026-01-03"),
        ];
        let tree = build_fork_tree(&sessions, "/f/p");
        assert_eq!(tree.children.len(), 3);
        let order: Vec<&str> =
            tree.children.iter().map(|c| c.meta.as_ref().unwrap().id.as_str()).collect();
        assert_eq!(order, vec!["c1", "c2", "c3"], "子应按 started_at 升序");
    }

    /// 多级链：祖→父→子→孙，从孙发起仍得完整链。
    #[test]
    fn multilevel_chain_full_depth() {
        let sessions = vec![
            s("g", None, "2026-01-01"),
            s("p", Some("g"), "2026-01-02"),
            s("c", Some("p"), "2026-01-03"),
            s("gc", Some("c"), "2026-01-04"),
        ];
        let tree = build_fork_tree(&sessions, "/f/gc");
        assert_eq!(tree.meta.as_ref().unwrap().id, "g");
        let mut got = vec![];
        ids(&tree, &mut got);
        got.sort();
        assert_eq!(got, vec!["c", "g", "gc", "p"]);
        assert_eq!(current_id(&tree).as_deref(), Some("gc"));
    }

    /// 缺父：forked_from 指向不在本地的父 → 链顶之上挂占位节点。
    #[test]
    fn missing_parent_gets_placeholder() {
        // child 的父 "ghost" 不在 sessions 中。
        let sessions = vec![s("child", Some("ghost"), "2026-01-02")];
        let tree = build_fork_tree(&sessions, "/f/child");
        assert!(tree.missing, "根应为缺父占位");
        assert!(tree.meta.is_none());
        assert_eq!(tree.children.len(), 1);
        assert_eq!(tree.children[0].meta.as_ref().unwrap().id, "child");
    }

    /// 多分支中只返回当前会话所属那条链（从其链顶展开）。
    #[test]
    fn returns_only_current_chain() {
        let sessions = vec![
            // 链 A: rootA -> a1
            s("rootA", None, "2026-01-01"),
            s("a1", Some("rootA"), "2026-01-02"),
            // 链 B: rootB -> b1（独立）
            s("rootB", None, "2026-01-01"),
            s("b1", Some("rootB"), "2026-01-02"),
        ];
        let tree = build_fork_tree(&sessions, "/f/a1");
        let mut got = vec![];
        ids(&tree, &mut got);
        got.sort();
        assert_eq!(got, vec!["a1", "rootA"], "不应混入链 B");
    }

    /// 环防御：自引用不致无限递归。
    #[test]
    fn self_cycle_does_not_loop() {
        // a 的父是自己（病态数据）。
        let sessions = vec![s("a", Some("a"), "2026-01-01")];
        let tree = build_fork_tree(&sessions, "/f/a");
        // 不崩溃即通过；根应可定位到 a。
        let mut got = vec![];
        ids(&tree, &mut got);
        assert!(got.contains(&"a".to_string()));
    }

    /// 当前会话不在集合 → 退化占位根，不 panic。
    #[test]
    fn current_not_found_degrades_gracefully() {
        let sessions = vec![s("a", None, "2026-01-01")];
        let tree = build_fork_tree(&sessions, "/f/zzz");
        assert!(tree.missing);
        assert!(tree.children.is_empty());
    }

    /// 前后端契约：meta=Some 时 serde flatten 把 SessionMeta 字段铺平到 ForkNode 顶层，
    /// 占位节点 missing=true 且无 id。前端 `ForkNode extends Partial<SessionMeta>` 依赖此结构。
    #[test]
    fn flatten_serialization_contract() {
        let sessions = vec![s("a", None, "2026-01-01")];
        let tree = build_fork_tree(&sessions, "/f/a");
        let v = serde_json::to_value(&tree).unwrap();
        // flatten：SessionMeta 的 id/title 应在顶层，而非嵌套在 meta 下
        assert_eq!(v["id"], "a", "flatten 应把 id 铺平到顶层");
        assert_eq!(v["is_current"], true);
        assert_eq!(v["missing"], false);
        assert!(v.get("meta").is_none(), "不应有嵌套 meta 字段");
        assert!(v["children"].is_array());
        // 占位节点：missing=true，无 id
        let placeholder = vec![s("c", Some("ghost"), "2026-01-01")];
        let t2 = build_fork_tree(&placeholder, "/f/c");
        let v2 = serde_json::to_value(&t2).unwrap();
        assert_eq!(v2["missing"], true);
        assert!(v2.get("id").is_none(), "占位节点无 id");
    }
}
