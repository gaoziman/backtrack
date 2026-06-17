//! SQLite 元数据缓存 + 子串搜索（CJK 友好，用 LIKE）。
use crate::models::{display_name_for, Project, SearchHit, SessionMeta, Tool};
use rusqlite::{params, Connection};

/// 短查询阈值：trigram 分词器要求 MATCH 查询 ≥ 3 字符，
/// 故 1-2 字（中文高频）走 LIKE 兜底，守住 CJK 红线。
const TRIGRAM_MIN: usize = 3;
/// 片段在命中词左右各保留的字符数。
const SNIPPET_RADIUS: usize = 24;

/// 单条会话正文最多缓存/索引的字符数。
/// 取 200K：覆盖绝大多数会话的对话正文，又能挡住病态超大会话；
/// 关键是约束 trigram 索引体积（trigram 按 3 字滑窗索引，体积对文本长度敏感）。
const BODY_CAP: usize = 200_000;

pub struct Store {
    pub conn: Connection,
    /// 运行环境是否支持 FTS5/trigram（建表成功才为 true）。
    has_fts: bool,
}

impl Store {
    pub fn open_in_memory() -> rusqlite::Result<Store> {
        let conn = Connection::open_in_memory()?;
        let mut s = Store { conn, has_fts: false };
        s.init_schema()?;
        Ok(s)
    }

    pub fn open(path: &std::path::Path) -> rusqlite::Result<Store> {
        let conn = Connection::open(path)?;
        let mut s = Store { conn, has_fts: false };
        s.init_schema()?;
        Ok(s)
    }

    fn init_schema(&mut self) -> rusqlite::Result<()> {
        // 旧库（缺 mtime 列）→ 丢弃 sessions/索引并重建。
        // 数据源自 jsonl 重扫，无损；避免逐列 ALTER 的兼容分支。
        if self.sessions_needs_rebuild()? {
            self.conn.execute_batch(
                "DROP TABLE IF EXISTS sessions_fts; DROP TABLE IF EXISTS sessions;",
            )?;
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id          TEXT NOT NULL,
                tool        TEXT NOT NULL,
                cwd         TEXT NOT NULL,
                file_path   TEXT NOT NULL PRIMARY KEY,
                title       TEXT NOT NULL,
                started_at  TEXT NOT NULL,
                updated_at  TEXT NOT NULL,
                msg_count   INTEGER NOT NULL,
                forked_from TEXT,
                body        TEXT NOT NULL,
                body_user   TEXT NOT NULL DEFAULT '',
                body_ai     TEXT NOT NULL DEFAULT '',
                mtime       INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_sessions_cwd ON sessions(cwd);
            CREATE INDEX IF NOT EXISTS idx_sessions_updated ON sessions(updated_at);
            CREATE TABLE IF NOT EXISTS hidden (cwd TEXT PRIMARY KEY);
            CREATE TABLE IF NOT EXISTS starred (cwd TEXT PRIMARY KEY);",
        )?;
        // FTS5 全文索引（trigram 子串分词，CJK 友好）。
        // 若运行环境的 SQLite 未编译 FTS5/trigram，则建表失败 → 记录并全程走 LIKE 兜底。
        self.has_fts = self
            .conn
            .execute_batch(
                "CREATE VIRTUAL TABLE IF NOT EXISTS sessions_fts USING fts5(
                    file_path UNINDEXED,
                    title,
                    body_user,
                    body_ai,
                    tokenize = 'trigram'
                );",
            )
            .is_ok();
        Ok(())
    }

    /// 检测既有 sessions 表是否缺少最新列（`mtime`），缺则需重建（旧版本库）。
    fn sessions_needs_rebuild(&self) -> rusqlite::Result<bool> {
        let exists: bool = self.conn.query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='sessions'",
            [],
            |_| Ok(true),
        ).unwrap_or(false);
        if !exists {
            return Ok(false); // 全新库，无需重建
        }
        let mut stmt = self.conn.prepare("PRAGMA table_info(sessions)")?;
        let cols: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(!cols.iter().any(|c| c == "mtime"))
    }

    /// 写入（或覆盖）一条会话。
    /// `body` 合并正文（LIKE 兜底 / 片段来源）；`body_user`/`body_ai` 供角色过滤。三者均自动截断。
    pub fn upsert(
        &self,
        m: &SessionMeta,
        body: &str,
        body_user: &str,
        body_ai: &str,
        mtime: i64,
    ) -> rusqlite::Result<()> {
        let cap = |s: &str| -> String { s.chars().take(BODY_CAP).collect() };
        let (capped, cu, ca) = (cap(body), cap(body_user), cap(body_ai));
        self.conn.execute(
            "INSERT OR REPLACE INTO sessions
             (id, tool, cwd, file_path, title, started_at, updated_at, msg_count, forked_from, body, body_user, body_ai, mtime)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![
                m.id, m.tool.as_str(), m.cwd, m.file_path, m.title,
                m.started_at, m.updated_at, m.message_count as i64, m.forked_from, capped, cu, ca, mtime
            ],
        )?;
        if self.has_fts {
            // 先删后插，保证覆盖语义与 INSERT OR REPLACE 对齐。
            self.conn
                .execute("DELETE FROM sessions_fts WHERE file_path = ?1", params![m.file_path])?;
            self.conn.execute(
                "INSERT INTO sessions_fts (file_path, title, body_user, body_ai)
                 VALUES (?1,?2,?3,?4)",
                params![m.file_path, m.title, cu, ca],
            )?;
        }
        Ok(())
    }

    /// 左栏：按 cwd 聚合的项目列表（排除隐藏），按会话数降序。
    pub fn list_projects(&self) -> rusqlite::Result<Vec<Project>> {
        let mut stmt = self.conn.prepare(
            "SELECT cwd, COUNT(*) FROM sessions
             WHERE cwd NOT IN (SELECT cwd FROM hidden)
             GROUP BY cwd ORDER BY COUNT(*) DESC, cwd",
        )?;
        let rows = stmt.query_map([], |r| {
            let cwd: String = r.get(0)?;
            let n: i64 = r.get(1)?;
            Ok(Project {
                display_name: display_name_for(&cwd),
                path: cwd,
                session_count: n as usize,
            })
        })?;
        rows.collect()
    }

    /// 已隐藏的目录（仍有会话的），用于侧栏「已隐藏」分组。
    pub fn list_hidden(&self) -> rusqlite::Result<Vec<Project>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.cwd, COUNT(*) FROM sessions s
             JOIN hidden h ON s.cwd = h.cwd
             GROUP BY s.cwd ORDER BY COUNT(*) DESC, s.cwd",
        )?;
        let rows = stmt.query_map([], |r| {
            let cwd: String = r.get(0)?;
            let n: i64 = r.get(1)?;
            Ok(Project {
                display_name: display_name_for(&cwd),
                path: cwd,
                session_count: n as usize,
            })
        })?;
        rows.collect()
    }

    /// 取某目录下全部会话文件路径（删除前用于移废纸篓）。
    pub fn paths_for_cwd(&self, cwd: &str) -> rusqlite::Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT file_path FROM sessions WHERE cwd = ?1")?;
        let rows = stmt.query_map(params![cwd], |r| r.get::<_, String>(0))?;
        rows.collect()
    }

    /// 从索引删除某目录的全部会话行，返回删除条数。
    pub fn delete_cwd(&self, cwd: &str) -> rusqlite::Result<usize> {
        if self.has_fts {
            self.conn.execute(
                "DELETE FROM sessions_fts WHERE file_path IN
                 (SELECT file_path FROM sessions WHERE cwd = ?1)",
                params![cwd],
            )?;
        }
        let n = self
            .conn
            .execute("DELETE FROM sessions WHERE cwd = ?1", params![cwd])?;
        self.conn
            .execute("DELETE FROM hidden WHERE cwd = ?1", params![cwd])?;
        self.conn
            .execute("DELETE FROM starred WHERE cwd = ?1", params![cwd])?;
        Ok(n)
    }

    /// 按文件路径删除若干会话行，返回删除条数。
    pub fn delete_paths(&self, paths: &[String]) -> rusqlite::Result<usize> {
        let mut n = 0;
        for p in paths {
            if self.has_fts {
                self.conn
                    .execute("DELETE FROM sessions_fts WHERE file_path = ?1", params![p])?;
            }
            n += self
                .conn
                .execute("DELETE FROM sessions WHERE file_path = ?1", params![p])?;
        }
        Ok(n)
    }

    pub fn hide(&self, cwd: &str) -> rusqlite::Result<()> {
        self.conn
            .execute("INSERT OR IGNORE INTO hidden(cwd) VALUES (?1)", params![cwd])?;
        Ok(())
    }

    pub fn unhide(&self, cwd: &str) -> rusqlite::Result<()> {
        self.conn
            .execute("DELETE FROM hidden WHERE cwd = ?1", params![cwd])?;
        Ok(())
    }

    /// 关注的目录路径列表。
    pub fn list_starred(&self) -> rusqlite::Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT cwd FROM starred")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect()
    }

    pub fn set_star(&self, cwd: &str, on: bool) -> rusqlite::Result<()> {
        if on {
            self.conn
                .execute("INSERT OR IGNORE INTO starred(cwd) VALUES (?1)", params![cwd])?;
        } else {
            self.conn
                .execute("DELETE FROM starred WHERE cwd = ?1", params![cwd])?;
        }
        Ok(())
    }

    /// 批量替换关注集合（管理面板「应用」用）。
    pub fn set_starred_all(&self, cwds: &[String]) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM starred", [])?;
        for c in cwds {
            self.conn
                .execute("INSERT OR IGNORE INTO starred(cwd) VALUES (?1)", params![c])?;
        }
        Ok(())
    }

    fn row_to_meta(r: &rusqlite::Row) -> rusqlite::Result<SessionMeta> {
        let tool_str: String = r.get("tool")?;
        let tool = Tool::from_str(&tool_str).unwrap_or(Tool::Claude);
        let id: String = r.get("id")?;
        Ok(SessionMeta {
            resume_command: tool.resume_command(&id),
            id,
            tool,
            cwd: r.get("cwd")?,
            file_path: r.get("file_path")?,
            title: r.get("title")?,
            started_at: r.get("started_at")?,
            updated_at: r.get("updated_at")?,
            message_count: r.get::<_, i64>("msg_count")? as usize,
            forked_from: r.get("forked_from")?,
        })
    }

    /// 中栏：某目录下的会话，按最近活动降序。
    pub fn list_sessions(&self, cwd: &str) -> rusqlite::Result<Vec<SessionMeta>> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM sessions WHERE cwd = ?1 ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map(params![cwd], Self::row_to_meta)?;
        rows.collect()
    }

    fn row_to_meta_body(r: &rusqlite::Row) -> rusqlite::Result<(SessionMeta, String)> {
        let meta = Self::row_to_meta(r)?;
        let body: String = r.get("body")?;
        Ok((meta, body))
    }

    /// 全局搜索（混合双轨）：
    /// - 查询 ≥ 3 字符且支持 FTS → trigram 相关度检索（rank 排序）；
    /// - 1-2 字符（CJK 高频）或无 FTS → LIKE 子串兜底（守 CJK 红线）。
    /// `role`: None|"all"|"user"|"ai"；`since`: ISO 时间下界（含），None=全部。
    /// 返回带命中正文片段的 `SearchHit`（仅标题命中时片段为 None）。
    pub fn search(
        &self,
        query: &str,
        role: Option<&str>,
        since: Option<&str>,
    ) -> rusqlite::Result<Vec<SearchHit>> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(vec![]);
        }
        let rows = if self.has_fts && q.chars().count() >= TRIGRAM_MIN {
            // FTS 若遇极端输入异常，兜底回退 LIKE，保证搜索永不因引擎报错而失败。
            match self.fts_search(q, role, since) {
                Ok(r) => r,
                Err(_) => self.like_search(q, role, since)?,
            }
        } else {
            self.like_search(q, role, since)?
        };
        Ok(rows
            .into_iter()
            .map(|(meta, body)| SearchHit {
                snippet: snippet_rust(&body, q, SNIPPET_RADIUS),
                meta,
            })
            .collect())
    }

    fn fts_search(
        &self,
        q: &str,
        role: Option<&str>,
        since: Option<&str>,
    ) -> rusqlite::Result<Vec<(SessionMeta, String)>> {
        let match_expr = fts_match_expr(role, q);
        // 注意：FTS5 的 MATCH 左操作数须为表名（非别名），rank 同理。
        let mut stmt = self.conn.prepare(
            "SELECT s.* FROM sessions_fts JOIN sessions s ON s.file_path = sessions_fts.file_path
             WHERE sessions_fts MATCH ?1 AND (?2 IS NULL OR s.updated_at >= ?2)
             ORDER BY sessions_fts.rank LIMIT 200",
        )?;
        let rows = stmt.query_map(params![match_expr, since], Self::row_to_meta_body)?;
        rows.collect()
    }

    fn like_search(
        &self,
        q: &str,
        role: Option<&str>,
        since: Option<&str>,
    ) -> rusqlite::Result<Vec<(SessionMeta, String)>> {
        let like = format!("%{}%", escape_like(q));
        let where_role = match role {
            Some("user") => "(title LIKE ?1 ESCAPE '\\' OR body_user LIKE ?1 ESCAPE '\\')",
            Some("ai") => "body_ai LIKE ?1 ESCAPE '\\'",
            _ => "(title LIKE ?1 ESCAPE '\\' OR body LIKE ?1 ESCAPE '\\')",
        };
        let sql = format!(
            "SELECT * FROM sessions WHERE {where_role}
             AND (?2 IS NULL OR updated_at >= ?2)
             ORDER BY updated_at DESC LIMIT 200"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![like, since], Self::row_to_meta_body)?;
        rows.collect()
    }

    pub fn count(&self) -> rusqlite::Result<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    /// 已索引的 `{文件路径 → mtime}` 映射，供增量同步比对。
    pub fn indexed_mtimes(&self) -> rusqlite::Result<std::collections::HashMap<String, i64>> {
        let mut stmt = self.conn.prepare("SELECT file_path, mtime FROM sessions")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        rows.collect()
    }
}

/// 把用户输入安全包装为 FTS5 字面 phrase，并按角色限定列。
/// 转义：内部 `"` → `""`，整体包 `"..."`，使 `*`/`:`/`(` 等 FTS 语法字符按字面匹配，杜绝语法报错与注入。
fn fts_match_expr(role: Option<&str>, q: &str) -> String {
    let phrase = format!("\"{}\"", q.replace('"', "\"\""));
    match role {
        Some("user") => format!("{{title body_user}} : {phrase}"),
        Some("ai") => format!("{{body_ai}} : {phrase}"),
        _ => phrase, // all：默认匹配全部已索引列
    }
}

/// 转义 LIKE 通配符，使 `%`/`_`/`\` 按字面匹配（配合 `ESCAPE '\'`）。
fn escape_like(q: &str) -> String {
    q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

/// 从合并正文里按 query 截取单行片段（命中词左右各 `radius` 字符，CJK 字符安全）。
/// 找不到（如仅标题命中）→ None。大小写不敏感（ASCII）。
fn snippet_rust(body: &str, q: &str, radius: usize) -> Option<String> {
    let lower_body = body.to_lowercase();
    let lower_q = q.to_lowercase();
    let byte_idx = lower_body.find(&lower_q)?;
    // ASCII/CJK 下原文与小写串字符数 1:1 对齐，用字符索引安全切片。
    let char_start = lower_body[..byte_idx].chars().count();
    let chars: Vec<char> = body.chars().collect();
    let q_len = q.chars().count();
    let from = char_start.saturating_sub(radius);
    let to = (char_start + q_len + radius).min(chars.len());
    let frag: String = chars[from..to].iter().collect();
    let frag = frag.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::new();
    if from > 0 {
        out.push('…');
    }
    out.push_str(&frag);
    if to < chars.len() {
        out.push('…');
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(id: &str, cwd: &str, title: &str, updated: &str) -> SessionMeta {
        SessionMeta {
            id: id.into(), tool: Tool::Claude, cwd: cwd.into(),
            file_path: format!("/f/{}", id), title: title.into(),
            started_at: "2026-01-01".into(), updated_at: updated.into(),
            message_count: 3, forked_from: None,
            resume_command: Tool::Claude.resume_command(id),
        }
    }

    /// 测试便捷写入：合并 body 同时作为 user 文本（role-agnostic 用例足够）。
    fn up(s: &Store, m: &SessionMeta, body: &str) {
        s.upsert(m, body, body, "", 0).unwrap();
    }

    /// 显式角色分列写入（角色过滤用例）。
    fn up_roles(s: &Store, m: &SessionMeta, body_user: &str, body_ai: &str) {
        let body = format!("{}\n{}", body_user, body_ai);
        s.upsert(m, &body, body_user, body_ai, 0).unwrap();
    }

    #[test]
    fn aggregates_projects_and_filters_sessions() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta("a", "/p/ai", "旅迹原型", "2026-06-16"), "建 stitch 项目 旅迹");
        up(&s, &meta("b", "/p/ai", "typora 主题", "2026-06-15"), "改造 juejin 主题");
        up(&s, &meta("c", "/p/hub", "登录 bug", "2026-06-14"), "oauth 回调");

        let projs = s.list_projects().unwrap();
        assert_eq!(projs.len(), 2);
        assert_eq!(projs[0].path, "/p/ai"); // 会话多的排前
        assert_eq!(projs[0].session_count, 2);

        let ai = s.list_sessions("/p/ai").unwrap();
        assert_eq!(ai.len(), 2);
        assert_eq!(ai[0].id, "a"); // 最近的排前
    }

    /// 红线回归：2 字中文（< trigram 阈值）必须经 LIKE 兜底命中。
    #[test]
    fn search_matches_cjk_substring() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta("a", "/p/ai", "旅迹小程序原型", "2026-06-16"), "情侣旅行回忆地图");
        up(&s, &meta("b", "/p/ai", "typora 主题", "2026-06-15"), "改造主题");

        let hits = s.search("旅迹", None, None).unwrap(); // 2 字 → LIKE
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].meta.id, "a");

        let body_hit = s.search("回忆地图", None, None).unwrap(); // 4 字 → FTS
        assert_eq!(body_hit.len(), 1);
    }

    /// ≥3 字命中应返回非空正文片段（含命中词）。
    #[test]
    fn search_returns_snippet_for_body_hit() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta("a", "/p/ai", "无关标题", "2026-06-16"),
           "前言铺垫很多很多文字 然后这里出现 登录失败 的关键内容 后面还有很多文字");

        let hits = s.search("登录失败", None, None).unwrap();
        assert_eq!(hits.len(), 1);
        let snip = hits[0].snippet.as_ref().expect("应有片段");
        assert!(snip.contains("登录失败"), "片段应含命中词: {snip}");
    }

    /// 仅标题命中（正文无该词）→ 片段为 None。
    #[test]
    fn search_title_only_hit_has_no_snippet() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta("a", "/p/ai", "重构布局方案讨论", "2026-06-16"), "完全无关的正文内容啊啊啊");
        let hits = s.search("重构布局", None, None).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].snippet.is_none());
    }

    /// 角色过滤：只搜 AI 文本 / 只搜用户文本。
    #[test]
    fn search_filters_by_role() {
        let s = Store::open_in_memory().unwrap();
        up_roles(&s, &meta("a", "/p/ai", "标题甲", "2026-06-16"),
                 "用户问怎么修复缓存穿透", "助手答用布隆过滤器处理穿透");

        // "布隆过滤器" 只在 AI 文本
        assert_eq!(s.search("布隆过滤器", Some("ai"), None).unwrap().len(), 1);
        assert_eq!(s.search("布隆过滤器", Some("user"), None).unwrap().len(), 0);
        // "怎么修复" 只在用户文本
        assert_eq!(s.search("怎么修复", Some("user"), None).unwrap().len(), 1);
        assert_eq!(s.search("怎么修复", Some("ai"), None).unwrap().len(), 0);
        // all 都能命中
        assert_eq!(s.search("布隆过滤器", Some("all"), None).unwrap().len(), 1);
    }

    /// 时间下界过滤。
    #[test]
    fn search_filters_by_since() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta("old", "/p/ai", "标题", "2026-01-01T00:00:00Z"), "共同关键词 检索目标");
        up(&s, &meta("new", "/p/ai", "标题", "2026-06-16T00:00:00Z"), "共同关键词 检索目标");

        let all = s.search("检索目标", None, None).unwrap();
        assert_eq!(all.len(), 2);
        let recent = s.search("检索目标", None, Some("2026-03-01")).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].meta.id, "new");
    }

    /// 安全：含 FTS 特殊字符的查询不报错（按字面匹配）。
    #[test]
    fn search_with_fts_special_chars_is_safe() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta("a", "/p/ai", "标题", "2026-06-16"), r#"包含 "引号" 与 a*b:c (括号) 的内容片段"#);
        // 这些查询若裸拼进 MATCH 会语法报错；应安全且按字面匹配
        assert!(s.search("a*b:c", None, None).is_ok());
        assert!(s.search(r#""引号""#, None, None).is_ok());
        assert_eq!(s.search("括号", None, None).unwrap().len(), 1);
    }

    /// 删除会话后，FTS 索引同步移除（不再被搜到）。
    #[test]
    fn delete_paths_also_updates_fts() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta("a", "/p/x", "标题", "2026-06-16"), "独特关键词 alpha 检索");
        assert_eq!(s.search("独特关键词", None, None).unwrap().len(), 1);
        s.delete_paths(&["/f/a".to_string()]).unwrap();
        assert_eq!(s.search("独特关键词", None, None).unwrap().len(), 0);
    }

    /// 环境自检：本机 bundled SQLite 应支持 FTS5/trigram。
    #[test]
    fn fts_is_available_in_this_env() {
        let s = Store::open_in_memory().unwrap();
        assert!(s.has_fts, "bundled rusqlite 应启用 FTS5；若失败说明环境缺 trigram");
    }

    #[test]
    fn hide_excludes_from_projects_and_lists_separately() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta("a", "/p/keep", "保留", "2026-06-16"), "x");
        up(&s, &meta("b", "/p/junk", "垃圾", "2026-06-15"), "y");

        s.hide("/p/junk").unwrap();
        let visible = s.list_projects().unwrap();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].path, "/p/keep");

        let hidden = s.list_hidden().unwrap();
        assert_eq!(hidden.len(), 1);
        assert_eq!(hidden[0].path, "/p/junk");

        s.unhide("/p/junk").unwrap();
        assert_eq!(s.list_projects().unwrap().len(), 2);
        assert!(s.list_hidden().unwrap().is_empty());
    }

    #[test]
    fn delete_cwd_removes_sessions_and_returns_paths() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta("a", "/p/x", "t1", "2026-06-16"), "x");
        up(&s, &meta("b", "/p/x", "t2", "2026-06-15"), "y");
        up(&s, &meta("c", "/p/other", "t3", "2026-06-14"), "z");

        let paths = s.paths_for_cwd("/p/x").unwrap();
        assert_eq!(paths.len(), 2);

        let n = s.delete_cwd("/p/x").unwrap();
        assert_eq!(n, 2);
        assert_eq!(s.count().unwrap(), 1);
        assert_eq!(s.list_projects().unwrap().len(), 1);
    }

    #[test]
    fn star_set_and_replace() {
        let s = Store::open_in_memory().unwrap();
        s.set_star("/p/a", true).unwrap();
        s.set_star("/p/b", true).unwrap();
        assert_eq!(s.list_starred().unwrap().len(), 2);

        s.set_star("/p/a", false).unwrap();
        assert_eq!(s.list_starred().unwrap(), vec!["/p/b".to_string()]);

        // 批量替换
        s.set_starred_all(&["/p/x".to_string(), "/p/y".to_string()]).unwrap();
        let mut got = s.list_starred().unwrap();
        got.sort();
        assert_eq!(got, vec!["/p/x".to_string(), "/p/y".to_string()]);
    }

    #[test]
    fn delete_paths_removes_specific_sessions() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta("a", "/p/x", "t1", "2026-06-16"), "x");
        up(&s, &meta("b", "/p/x", "t2", "2026-06-15"), "y");
        up(&s, &meta("c", "/p/x", "t3", "2026-06-14"), "z");

        // meta() 的 file_path 形如 /f/<id>
        let n = s.delete_paths(&["/f/a".to_string(), "/f/c".to_string()]).unwrap();
        assert_eq!(n, 2);
        assert_eq!(s.count().unwrap(), 1);
        let left = s.list_sessions("/p/x").unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id, "b");
    }

    #[test]
    fn upsert_is_idempotent_by_path() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta("a", "/p", "t", "2026-06-16"), "x");
        up(&s, &meta("a", "/p", "t2", "2026-06-17"), "x");
        assert_eq!(s.count().unwrap(), 1);
    }
}
