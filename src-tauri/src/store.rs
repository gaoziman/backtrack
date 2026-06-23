//! SQLite 元数据缓存 + 子串搜索（CJK 友好，用 LIKE）。
use crate::models::{
    display_name_for, Collection, DayCount, DirCount, MonthCount, Project, SearchHit, SessionMeta,
    StatsDto, Tool, ToolCount,
};
use rusqlite::{params, Connection, OptionalExtension};

/// 当前时间字符串（收藏/分类的 created_at 用，仅记录性质，格式与 ai_summary 对齐）。
fn now_string() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

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
            CREATE TABLE IF NOT EXISTS starred (cwd TEXT PRIMARY KEY);
            CREATE TABLE IF NOT EXISTS custom_titles (file_path TEXT PRIMARY KEY, title TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS ai_titles (file_path TEXT PRIMARY KEY, title TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS ai_summaries (
                file_path  TEXT PRIMARY KEY,
                summary    TEXT NOT NULL,
                model      TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS collections (
                id         TEXT PRIMARY KEY,
                name       TEXT NOT NULL,
                color      TEXT NOT NULL DEFAULT 'slate',
                sort       INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS favorites (
                file_path     TEXT NOT NULL,
                collection_id TEXT NOT NULL DEFAULT '',
                created_at    TEXT NOT NULL DEFAULT '',
                PRIMARY KEY (file_path, collection_id)
            );
            CREATE INDEX IF NOT EXISTS idx_fav_collection ON favorites(collection_id);
            CREATE INDEX IF NOT EXISTS idx_fav_path ON favorites(file_path);",
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

    /// 全局使用统计：一次性聚合统计面板所需的全部维度（只读、纯聚合）。
    /// 时间维度直接用 `started_at` ISO 字符串前缀切片（`substr`），无需解析。
    pub fn stats(&self) -> rusqlite::Result<StatsDto> {
        // 总量、最早/最近、目录数、fork 数、正文字符数（一行扫描搞定）。
        let (total_sessions, total_messages, distinct_dirs, fork_count, total_body_chars, earliest, latest) =
            self.conn.query_row(
                "SELECT
                    COUNT(*),
                    COALESCE(SUM(msg_count), 0),
                    COUNT(DISTINCT cwd),
                    COALESCE(SUM(forked_from IS NOT NULL), 0),
                    COALESCE(SUM(LENGTH(body)), 0),
                    MIN(started_at),
                    MAX(updated_at)
                 FROM sessions",
                [],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)? as usize,
                        r.get::<_, i64>(1)? as usize,
                        r.get::<_, i64>(2)? as usize,
                        r.get::<_, i64>(3)? as usize,
                        r.get::<_, i64>(4)? as usize,
                        r.get::<_, Option<String>>(5)?,
                        r.get::<_, Option<String>>(6)?,
                    ))
                },
            )?;

        // 按工具计数（降序）。
        let mut stmt = self.conn.prepare(
            "SELECT tool, COUNT(*) FROM sessions GROUP BY tool ORDER BY COUNT(*) DESC, tool",
        )?;
        let by_tool = stmt
            .query_map([], |r| {
                let t: String = r.get(0)?;
                let n: i64 = r.get(1)?;
                Ok((t, n as usize))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            // 解析失败的工具行直接跳过（理论上不会出现）。
            .filter_map(|(t, count)| Tool::from_str(&t).map(|tool| ToolCount { tool, count }))
            .collect();

        // 按月计数（升序），月份 = started_at 前 7 字符（YYYY-MM）。
        let mut stmt = self.conn.prepare(
            "SELECT substr(started_at, 1, 7) m, COUNT(*) FROM sessions
             GROUP BY m ORDER BY m",
        )?;
        let by_month = stmt
            .query_map([], |r| {
                Ok(MonthCount { month: r.get::<_, String>(0)?, count: r.get::<_, i64>(1)? as usize })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        // 按天计数（升序），日 = started_at 前 10 字符（YYYY-MM-DD）。
        let mut stmt = self.conn.prepare(
            "SELECT substr(started_at, 1, 10) d, COUNT(*) FROM sessions
             GROUP BY d ORDER BY d",
        )?;
        let by_day = stmt
            .query_map([], |r| {
                Ok(DayCount { day: r.get::<_, String>(0)?, count: r.get::<_, i64>(1)? as usize })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        // 最活跃目录（降序）。
        let mut stmt = self.conn.prepare(
            "SELECT cwd, COUNT(*) FROM sessions GROUP BY cwd ORDER BY COUNT(*) DESC, cwd",
        )?;
        let top_dirs = stmt
            .query_map([], |r| {
                let cwd: String = r.get(0)?;
                let n: i64 = r.get(1)?;
                Ok(DirCount {
                    display_name: display_name_for(&cwd),
                    cwd,
                    count: n as usize,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(StatsDto {
            total_sessions,
            total_messages,
            total_body_chars,
            distinct_dirs,
            fork_count,
            earliest,
            latest,
            by_tool,
            by_month,
            by_day,
            top_dirs,
        })
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
        self.conn.execute(
            "DELETE FROM custom_titles WHERE file_path IN
             (SELECT file_path FROM sessions WHERE cwd = ?1)",
            params![cwd],
        )?;
        self.conn.execute(
            "DELETE FROM ai_titles WHERE file_path IN
             (SELECT file_path FROM sessions WHERE cwd = ?1)",
            params![cwd],
        )?;
        self.conn.execute(
            "DELETE FROM ai_summaries WHERE file_path IN
             (SELECT file_path FROM sessions WHERE cwd = ?1)",
            params![cwd],
        )?;
        self.conn.execute(
            "DELETE FROM favorites WHERE file_path IN
             (SELECT file_path FROM sessions WHERE cwd = ?1)",
            params![cwd],
        )?;
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
            self.conn
                .execute("DELETE FROM custom_titles WHERE file_path = ?1", params![p])?;
            self.conn
                .execute("DELETE FROM ai_titles WHERE file_path = ?1", params![p])?;
            self.conn
                .execute("DELETE FROM ai_summaries WHERE file_path = ?1", params![p])?;
            self.conn
                .execute("DELETE FROM favorites WHERE file_path = ?1", params![p])?;
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

    // ============ 收藏 + 分类（Collections） ============
    // favorites.collection_id 用空串 '' 作「未分类」哨兵（NULL 在 PK 中互不相等，故不用 NULL）。
    // favorited = 该 file_path 在 favorites 表有任意行；collection_ids = 其下非空 collection_id 集合。

    /// 取并自增一个单调序列值（分类 id 生成用，存于 meta 表，唯一且不与删除冲突）。
    fn next_seq(&self, key: &str) -> rusqlite::Result<i64> {
        let cur: i64 = self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let next = cur + 1;
        self.conn.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES (?1, ?2)",
            params![key, next.to_string()],
        )?;
        Ok(next)
    }

    /// 列出全部分类（按 sort 升序），附带每类收藏计数。
    pub fn list_collections(&self) -> rusqlite::Result<Vec<Collection>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.id, c.name, c.color, c.sort,
                    (SELECT COUNT(*) FROM favorites f WHERE f.collection_id = c.id) AS cnt
             FROM collections c ORDER BY c.sort ASC, c.id ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Collection {
                id: r.get(0)?,
                name: r.get(1)?,
                color: r.get(2)?,
                sort: r.get(3)?,
                count: r.get::<_, i64>(4)? as usize,
            })
        })?;
        rows.collect()
    }

    /// 新建分类，sort 取当前最大 sort + 1（追加到末尾）。
    pub fn create_collection(&self, name: &str, color: &str) -> rusqlite::Result<Collection> {
        let id = format!("col_{}", self.next_seq("collection_seq")?);
        let sort: i64 = self
            .conn
            .query_row("SELECT COALESCE(MAX(sort), 0) FROM collections", [], |r| r.get(0))
            .unwrap_or(0)
            + 1;
        let created_at = now_string();
        self.conn.execute(
            "INSERT INTO collections(id, name, color, sort, created_at) VALUES (?1,?2,?3,?4,?5)",
            params![id, name.trim(), color, sort, created_at],
        )?;
        Ok(Collection { id, name: name.trim().to_string(), color: color.to_string(), sort, count: 0 })
    }

    /// 改名 + 改色。
    pub fn rename_collection(&self, id: &str, name: &str, color: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE collections SET name = ?2, color = ?3 WHERE id = ?1",
            params![id, name.trim(), color],
        )?;
        Ok(())
    }

    /// 删除分类：该分类下的收藏降级为「未分类」（不丢收藏），再删分类定义。
    pub fn delete_collection(&self, id: &str) -> rusqlite::Result<()> {
        // 仅为「除本分类外已无其它收藏行」的会话补未分类哨兵，保住「已收藏」；
        // 仍属其它分类的会话不补哨兵，避免遗留冗余 '' 行。
        self.conn.execute(
            "INSERT OR IGNORE INTO favorites(file_path, collection_id, created_at)
             SELECT file_path, '', '' FROM favorites
             WHERE collection_id = ?1
               AND file_path NOT IN (
                 SELECT file_path FROM favorites WHERE collection_id <> ?1 AND collection_id <> ''
               )",
            params![id],
        )?;
        self.conn
            .execute("DELETE FROM favorites WHERE collection_id = ?1", params![id])?;
        self.conn
            .execute("DELETE FROM collections WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// 按给定顺序重排分类（拖拽排序用）：依次写 sort = 0,1,2,…
    pub fn reorder_collections(&self, ids: &[String]) -> rusqlite::Result<()> {
        for (i, id) in ids.iter().enumerate() {
            self.conn.execute(
                "UPDATE collections SET sort = ?2 WHERE id = ?1",
                params![id, i as i64],
            )?;
        }
        Ok(())
    }

    /// 收藏 / 取消收藏一个会话，并（在收藏时）设置其所属分类（覆盖语义）。
    /// `on=false`：删除该会话全部收藏行。
    /// `on=true` 且 `collection_ids` 非空：精确归入这些分类（先清旧、再插新）。
    /// `on=true` 且 `collection_ids` 为空：仅收藏不归类（写未分类哨兵行）。
    pub fn set_favorite(
        &self,
        file_path: &str,
        collection_ids: &[String],
        on: bool,
    ) -> rusqlite::Result<()> {
        // 先清除该会话现有全部收藏行（覆盖语义，保证幂等）。
        self.conn
            .execute("DELETE FROM favorites WHERE file_path = ?1", params![file_path])?;
        if !on {
            return Ok(());
        }
        let created_at = now_string();
        if collection_ids.is_empty() {
            self.conn.execute(
                "INSERT OR IGNORE INTO favorites(file_path, collection_id, created_at) VALUES (?1, '', ?2)",
                params![file_path, created_at],
            )?;
        } else {
            for cid in collection_ids {
                self.conn.execute(
                    "INSERT OR IGNORE INTO favorites(file_path, collection_id, created_at) VALUES (?1, ?2, ?3)",
                    params![file_path, cid, created_at],
                )?;
            }
        }
        Ok(())
    }

    /// 收藏视图数据：可按分类筛（None=全部收藏），可叠加搜索（None/空=不搜）。
    /// 每条 overlay favorited=true + collection_ids（所属分类，非空哨兵）。按 updated_at 降序。
    pub fn list_favorites(
        &self,
        collection_id: Option<&str>,
        query: Option<&str>,
    ) -> rusqlite::Result<Vec<SessionMeta>> {
        let q = query.map(str::trim).filter(|s| !s.is_empty());
        // 收藏会话集合：按分类筛则限定该 collection_id，否则取 favorites 中全部 distinct file_path。
        let (fav_filter, fav_param): (&str, Option<String>) = match collection_id {
            Some(cid) => ("WHERE f.collection_id = ?1", Some(cid.to_string())),
            None => ("", None),
        };
        // 搜索:对标题/正文 LIKE 子串(收藏视图数据量小,统一走 LIKE 即可,CJK 友好)。
        // 复用 escape_like：先转义反斜杠再转义 %/_，避免查询含 `\` 时 ESCAPE 误判（与 like_search 一致）。
        let like = q.map(|s| format!("%{}%", escape_like(s)));
        let sql = format!(
            "SELECT s.id, s.tool, s.cwd, s.file_path,
                    COALESCE(ct.title, at.title, s.title) AS title,
                    s.started_at, s.updated_at, s.msg_count, s.forked_from
             FROM sessions s
             JOIN (SELECT DISTINCT f.file_path FROM favorites f {fav_filter}) fv
                  ON fv.file_path = s.file_path
             LEFT JOIN custom_titles ct ON ct.file_path = s.file_path
             LEFT JOIN ai_titles at ON at.file_path = s.file_path
             {search_clause}
             ORDER BY s.updated_at DESC",
            search_clause = if like.is_some() {
                "WHERE (COALESCE(ct.title, at.title, s.title) LIKE ?S ESCAPE '\\' OR s.body LIKE ?S ESCAPE '\\')"
            } else {
                ""
            }
        );
        // 组装参数：fav_param（若有）在前，like（若有）替换占位。
        let mut metas: Vec<SessionMeta> = {
            // rusqlite 不支持命名重复占位，手工按出现顺序绑定。
            let sql = sql.replace("?S", if fav_param.is_some() { "?2" } else { "?1" });
            let mut stmt = self.conn.prepare(&sql)?;
            let map = Self::row_to_meta;
            match (fav_param.as_ref(), like.as_ref()) {
                (Some(fp), Some(lk)) => stmt.query_map(params![fp, lk], map)?.collect::<rusqlite::Result<Vec<_>>>()?,
                (Some(fp), None) => stmt.query_map(params![fp], map)?.collect::<rusqlite::Result<Vec<_>>>()?,
                (None, Some(lk)) => stmt.query_map(params![lk], map)?.collect::<rusqlite::Result<Vec<_>>>()?,
                (None, None) => stmt.query_map([], map)?.collect::<rusqlite::Result<Vec<_>>>()?,
            }
        };
        // overlay favorited + collection_ids（一次性批量取所有归类，避免 per-row N+1）。
        let mut by_path = self.collection_ids_by_path()?;
        for m in metas.iter_mut() {
            m.favorited = true;
            m.collection_ids = by_path.remove(&m.file_path).unwrap_or_default();
        }
        Ok(metas)
    }

    /// 一次性取全部「会话→所属分类 id 列表」映射（排除未分类哨兵 ''，按分类 sort 升序）。
    /// 单次查询，供 list_favorites 批量 overlay，避免逐会话查询的 N+1。
    fn collection_ids_by_path(&self) -> rusqlite::Result<std::collections::HashMap<String, Vec<String>>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.file_path, f.collection_id FROM favorites f
             JOIN collections c ON c.id = f.collection_id
             WHERE f.collection_id <> ''
             ORDER BY c.sort ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
        for row in rows {
            let (path, cid) = row?;
            map.entry(path).or_default().push(cid);
        }
        Ok(map)
    }


    /// 设置会话自定义标题（独立持久化，读时 override 派生标题，不被增量重索引覆盖）。
    pub fn set_custom_title(&self, file_path: &str, title: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO custom_titles(file_path, title) VALUES (?1, ?2)",
            params![file_path, title],
        )?;
        Ok(())
    }

    /// 清除自定义标题，恢复为派生标题。
    pub fn clear_custom_title(&self, file_path: &str) -> rusqlite::Result<()> {
        self.conn
            .execute("DELETE FROM custom_titles WHERE file_path = ?1", params![file_path])?;
        Ok(())
    }

    /// 读取派生标题（恢复默认时回读返回给前端）。
    pub fn derived_title(&self, file_path: &str) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT title FROM sessions WHERE file_path = ?1",
                params![file_path],
                |r| r.get::<_, String>(0),
            )
            .optional()
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
            // 默认 false；list_sessions 用专用查询覆盖为真实值。
            has_children: false,
            // 默认 false/空；list_sessions/list_favorites 用专用查询 overlay。
            favorited: false,
            collection_ids: Vec::new(),
        })
    }

    /// 中栏：某目录下的会话，按最近活动降序。
    /// 附带计算 `has_children`（是否有其它会话 fork 自本会话），供前端判定谱系入口。
    pub fn list_sessions(&self, cwd: &str) -> rusqlite::Result<Vec<SessionMeta>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.tool, s.cwd, s.file_path,
                    COALESCE(ct.title, at.title, s.title) AS title,
                    s.started_at, s.updated_at, s.msg_count, s.forked_from,
                    EXISTS(SELECT 1 FROM sessions f WHERE f.forked_from = s.id) AS has_kids,
                    EXISTS(SELECT 1 FROM favorites fv WHERE fv.file_path = s.file_path) AS is_fav
             FROM sessions s LEFT JOIN custom_titles ct ON ct.file_path = s.file_path
             LEFT JOIN ai_titles at ON at.file_path = s.file_path
             WHERE s.cwd = ?1 ORDER BY s.updated_at DESC",
        )?;
        let rows = stmt.query_map(params![cwd], Self::row_to_meta_with_children)?;
        rows.collect()
    }

    /// 按文件路径取单条会话元数据（fork_tree 命令定位当前会话用）。
    pub fn session_by_path(&self, file_path: &str) -> rusqlite::Result<Option<SessionMeta>> {
        self.conn
            .query_row(
                "SELECT s.id, s.tool, s.cwd, s.file_path,
                        COALESCE(ct.title, at.title, s.title) AS title,
                        s.started_at, s.updated_at, s.msg_count, s.forked_from
                 FROM sessions s LEFT JOIN custom_titles ct ON ct.file_path = s.file_path
             LEFT JOIN ai_titles at ON at.file_path = s.file_path
                 WHERE s.file_path = ?1",
                params![file_path],
                Self::row_to_meta,
            )
            .optional()
    }

    /// 同 row_to_meta，但额外读取 `has_kids`/`is_fav` 列填充 has_children/favorited。
    fn row_to_meta_with_children(r: &rusqlite::Row) -> rusqlite::Result<SessionMeta> {
        let mut m = Self::row_to_meta(r)?;
        m.has_children = r.get::<_, bool>("has_kids")?;
        m.favorited = r.get::<_, bool>("is_fav")?;
        Ok(m)
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
        self.search_filtered(query, role, since, None, None)
    }

    /// 全文搜索 + 多维过滤（role/since/tools/cwd 全部 AND，后端完成）。
    /// `tools`: 选中的工具列表；None 或含全部(>=2)=不过滤工具。
    /// `cwd`: 目录过滤；None/空=不过滤。
    pub fn search_filtered(
        &self,
        query: &str,
        role: Option<&str>,
        since: Option<&str>,
        tools: Option<&[String]>,
        cwd: Option<&str>,
    ) -> rusqlite::Result<Vec<SearchHit>> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(vec![]);
        }
        let rows = if self.has_fts && q.chars().count() >= TRIGRAM_MIN {
            // FTS 若遇极端输入异常，兜底回退 LIKE，保证搜索永不因引擎报错而失败。
            match self.fts_search(q, role, since, tools, cwd) {
                Ok(r) => r,
                Err(_) => self.like_search(q, role, since, tools, cwd)?,
            }
        } else {
            self.like_search(q, role, since, tools, cwd)?
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
        tools: Option<&[String]>,
        cwd: Option<&str>,
    ) -> rusqlite::Result<Vec<(SessionMeta, String)>> {
        let match_expr = fts_match_expr(role, q);
        let (filter_sql, filter_params) = build_filters(tools, cwd);
        // 注意：FTS5 的 MATCH 左操作数须为表名（非别名），rank 同理。
        let sql = format!(
            "SELECT s.id, s.tool, s.cwd, s.file_path,
                    COALESCE(ct.title, at.title, s.title) AS title,
                    s.started_at, s.updated_at, s.msg_count, s.forked_from, s.body
             FROM sessions_fts
             JOIN sessions s ON s.file_path = sessions_fts.file_path
             LEFT JOIN custom_titles ct ON ct.file_path = s.file_path
             LEFT JOIN ai_titles at ON at.file_path = s.file_path
             WHERE sessions_fts MATCH ? AND (? IS NULL OR s.updated_at >= ?){filter_sql}
             ORDER BY sessions_fts.rank LIMIT 200"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        // 参数顺序：match_expr, since, since, 然后 filter 参数
        let mut params: Vec<&dyn rusqlite::ToSql> = vec![&match_expr, &since, &since];
        for p in &filter_params {
            params.push(p.as_ref());
        }
        let rows = stmt.query_map(params.as_slice(), Self::row_to_meta_body)?;
        rows.collect()
    }

    fn like_search(
        &self,
        q: &str,
        role: Option<&str>,
        since: Option<&str>,
        tools: Option<&[String]>,
        cwd: Option<&str>,
    ) -> rusqlite::Result<Vec<(SessionMeta, String)>> {
        let like = format!("%{}%", escape_like(q));
        let where_role = match role {
            Some("user") => "(s.title LIKE ? ESCAPE '\\' OR s.body_user LIKE ? ESCAPE '\\')",
            Some("ai") => "s.body_ai LIKE ? ESCAPE '\\'",
            _ => "(s.title LIKE ? ESCAPE '\\' OR s.body LIKE ? ESCAPE '\\')",
        };
        let role_param_count = if matches!(role, Some("ai")) { 1 } else { 2 };
        let (filter_sql, filter_params) = build_filters(tools, cwd);
        let sql = format!(
            "SELECT s.id, s.tool, s.cwd, s.file_path,
                    COALESCE(ct.title, at.title, s.title) AS title,
                    s.started_at, s.updated_at, s.msg_count, s.forked_from, s.body
             FROM sessions s LEFT JOIN custom_titles ct ON ct.file_path = s.file_path
             LEFT JOIN ai_titles at ON at.file_path = s.file_path
             WHERE {where_role}
             AND (? IS NULL OR s.updated_at >= ?){filter_sql}
             ORDER BY s.updated_at DESC LIMIT 200"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        // 参数顺序：like × role_param_count, since, since, 然后 filter 参数
        let mut params: Vec<&dyn rusqlite::ToSql> = vec![];
        for _ in 0..role_param_count {
            params.push(&like);
        }
        params.push(&since);
        params.push(&since);
        for p in &filter_params {
            params.push(p.as_ref());
        }
        let rows = stmt.query_map(params.as_slice(), Self::row_to_meta_body)?;
        rows.collect()
    }

    pub fn count(&self) -> rusqlite::Result<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    /// 读标题逻辑版本号（meta 表，缺省 0）。用于判断 derive_title 升级后是否需全量重建。
    pub fn title_logic_version(&self) -> rusqlite::Result<i64> {
        let v: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'title_logic_version'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        Ok(v.and_then(|s| s.parse().ok()).unwrap_or(0))
    }

    /// 写标题逻辑版本号。
    pub fn set_title_logic_version(&self, v: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES ('title_logic_version', ?1)",
            params![v.to_string()],
        )?;
        Ok(())
    }

    /// 读 meta 表单个键（缺省 None）。
    fn meta_get(&self, key: &str) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| r.get(0))
            .optional()
    }

    /// 写 meta 表单个键。
    fn meta_set(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    /// 读取 AI 配置（默认关闭、空 key）。
    pub fn ai_config(&self) -> rusqlite::Result<crate::ai::AiConfig> {
        Ok(crate::ai::AiConfig {
            enabled: self.meta_get("ai_enabled")?.as_deref() == Some("1"),
            base_url: self.meta_get("ai_base_url")?.unwrap_or_default(),
            api_key: self.meta_get("ai_api_key")?.unwrap_or_default(),
            model: self.meta_get("ai_model")?.unwrap_or_else(|| "claude-opus-4-8".into()),
        })
    }

    /// 写入 AI 配置。
    pub fn set_ai_config(&self, cfg: &crate::ai::AiConfig) -> rusqlite::Result<()> {
        self.meta_set("ai_enabled", if cfg.enabled { "1" } else { "0" })?;
        self.meta_set("ai_base_url", cfg.base_url.trim())?;
        self.meta_set("ai_api_key", cfg.api_key.trim())?;
        self.meta_set("ai_model", cfg.model.trim())?;
        Ok(())
    }

    /// 读 AI 标题缓存。
    pub fn ai_title(&self, file_path: &str) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT title FROM ai_titles WHERE file_path = ?1",
                params![file_path],
                |r| r.get(0),
            )
            .optional()
    }

    /// 写 AI 标题缓存。
    pub fn set_ai_title(&self, file_path: &str, title: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO ai_titles(file_path, title) VALUES (?1, ?2)",
            params![file_path, title],
        )?;
        Ok(())
    }

    /// 读 AI 摘要缓存（反序列化为 AiSummary）。无缓存或解析失败 → None。
    pub fn ai_summary(&self, file_path: &str) -> rusqlite::Result<Option<crate::ai::AiSummary>> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT summary FROM ai_summaries WHERE file_path = ?1",
                params![file_path],
                |r| r.get(0),
            )
            .optional()?;
        Ok(json.and_then(|s| serde_json::from_str(&s).ok()))
    }

    /// 写 AI 摘要缓存（序列化为 JSON）。
    pub fn set_ai_summary(
        &self,
        file_path: &str,
        summary: &crate::ai::AiSummary,
        model: &str,
        created_at: &str,
    ) -> rusqlite::Result<()> {
        let json = serde_json::to_string(summary).unwrap_or_default();
        self.conn.execute(
            "INSERT OR REPLACE INTO ai_summaries(file_path, summary, model, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![file_path, json, model, created_at],
        )?;
        Ok(())
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

/// 构造 tool/cwd 过滤的 SQL 片段（追加到 WHERE 后）+ 对应参数（全部 ? 占位，防注入）。
/// tools 为 None 或包含 >=2 项（全选）时不过滤工具；cwd 为 None/空时不过滤目录。
fn build_filters(
    tools: Option<&[String]>,
    cwd: Option<&str>,
) -> (String, Vec<Box<dyn rusqlite::ToSql>>) {
    let mut sql = String::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(ts) = tools {
        // 只在恰好选 1 个工具时过滤；0 个或 >=2 个（全选）= 不限制。
        if ts.len() == 1 {
            sql.push_str(" AND s.tool = ?");
            params.push(Box::new(ts[0].clone()));
        }
    }
    if let Some(c) = cwd {
        if !c.is_empty() {
            sql.push_str(" AND s.cwd = ?");
            params.push(Box::new(c.to_string()));
        }
    }
    (sql, params)
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
        meta_tool(id, cwd, title, updated, Tool::Claude)
    }

    fn meta_tool(id: &str, cwd: &str, title: &str, updated: &str, tool: Tool) -> SessionMeta {
        SessionMeta {
            id: id.into(), tool, cwd: cwd.into(),
            file_path: format!("/f/{}", id), title: title.into(),
            started_at: "2026-01-01".into(), updated_at: updated.into(),
            message_count: 3, forked_from: None,
            resume_command: tool.resume_command(id),
            has_children: false,
            favorited: false, collection_ids: Vec::new(),
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

    /// 统计测试专用：可控制 tool / cwd / started_at / updated_at / forked_from。
    fn meta_full(
        id: &str, cwd: &str, tool: Tool, started: &str, updated: &str, forked: Option<&str>,
    ) -> SessionMeta {
        SessionMeta {
            id: id.into(), tool, cwd: cwd.into(),
            file_path: format!("/f/{}", id), title: id.into(),
            started_at: started.into(), updated_at: updated.into(),
            message_count: 10, forked_from: forked.map(|s| s.into()),
            resume_command: tool.resume_command(id),
            has_children: false,
            favorited: false, collection_ids: Vec::new(),
        }
    }

    #[test]
    fn stats_empty_db() {
        let s = Store::open_in_memory().unwrap();
        let st = s.stats().unwrap();
        assert_eq!(st.total_sessions, 0);
        assert_eq!(st.total_messages, 0);
        assert_eq!(st.distinct_dirs, 0);
        assert_eq!(st.fork_count, 0);
        assert_eq!(st.earliest, None);
        assert_eq!(st.latest, None);
        assert!(st.by_tool.is_empty());
        assert!(st.by_month.is_empty());
        assert!(st.by_day.is_empty());
        assert!(st.top_dirs.is_empty());
    }

    #[test]
    fn stats_totals_and_tools() {
        let s = Store::open_in_memory().unwrap();
        // 3 claude + 2 codex，跨两个目录；其中 1 个是 fork。
        up(&s, &meta_full("a", "/p/ai", Tool::Claude, "2026-04-01T00:00:00Z", "2026-04-01T00:00:00Z", None), "正文");
        up(&s, &meta_full("b", "/p/ai", Tool::Claude, "2026-05-02T00:00:00Z", "2026-05-02T00:00:00Z", None), "正文");
        up(&s, &meta_full("c", "/p/ai", Tool::Claude, "2026-06-03T00:00:00Z", "2026-06-03T00:00:00Z", None), "正文");
        up(&s, &meta_full("d", "/p/hub", Tool::Codex, "2026-06-04T00:00:00Z", "2026-06-04T00:00:00Z", Some("a")), "正文");
        up(&s, &meta_full("e", "/p/hub", Tool::Codex, "2026-06-05T00:00:00Z", "2026-06-09T00:00:00Z", None), "正文");

        let st = s.stats().unwrap();
        assert_eq!(st.total_sessions, 5);
        assert_eq!(st.total_messages, 50); // 5 × msg_count(10)
        assert_eq!(st.distinct_dirs, 2);
        assert_eq!(st.fork_count, 1);
        assert_eq!(st.earliest.as_deref(), Some("2026-04-01T00:00:00Z"));
        assert_eq!(st.latest.as_deref(), Some("2026-06-09T00:00:00Z"));

        // 工具占比：claude 3 在前（降序）。
        assert_eq!(st.by_tool.len(), 2);
        assert_eq!(st.by_tool[0], ToolCount { tool: Tool::Claude, count: 3 });
        assert_eq!(st.by_tool[1], ToolCount { tool: Tool::Codex, count: 2 });
    }

    #[test]
    fn stats_by_month_and_day_ascending() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta_full("a", "/p/ai", Tool::Claude, "2026-06-16T01:00:00Z", "x", None), "正文");
        up(&s, &meta_full("b", "/p/ai", Tool::Claude, "2026-06-16T09:00:00Z", "x", None), "正文");
        up(&s, &meta_full("c", "/p/ai", Tool::Claude, "2026-04-10T00:00:00Z", "x", None), "正文");

        let st = s.stats().unwrap();
        // 按月升序：4 月 1 条、6 月 2 条。
        assert_eq!(st.by_month, vec![
            MonthCount { month: "2026-04".into(), count: 1 },
            MonthCount { month: "2026-06".into(), count: 2 },
        ]);
        // 按天升序：04-10 一条、06-16 两条（同日合并）。
        assert_eq!(st.by_day, vec![
            DayCount { day: "2026-04-10".into(), count: 1 },
            DayCount { day: "2026-06-16".into(), count: 2 },
        ]);
    }

    #[test]
    fn stats_top_dirs_descending() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta_full("a", "/p/hub", Tool::Claude, "2026-06-01T00:00:00Z", "x", None), "正文");
        up(&s, &meta_full("b", "/p/hub", Tool::Claude, "2026-06-02T00:00:00Z", "x", None), "正文");
        up(&s, &meta_full("c", "/p/hub", Tool::Claude, "2026-06-03T00:00:00Z", "x", None), "正文");
        up(&s, &meta_full("d", "/p/ai", Tool::Codex, "2026-06-04T00:00:00Z", "x", None), "正文");

        let st = s.stats().unwrap();
        assert_eq!(st.top_dirs.len(), 2);
        assert_eq!(st.top_dirs[0].cwd, "/p/hub");
        assert_eq!(st.top_dirs[0].count, 3);
        assert_eq!(st.top_dirs[0].display_name, display_name_for("/p/hub"));
        assert_eq!(st.top_dirs[1].cwd, "/p/ai");
        assert_eq!(st.top_dirs[1].count, 1);
    }

    #[test]
    fn stats_body_chars_sums_merged_body() {
        let s = Store::open_in_memory().unwrap();
        // 中文每字 1 char；"正文内容" = 4 chars，两条 = 8。
        up(&s, &meta_full("a", "/p/ai", Tool::Claude, "2026-06-01T00:00:00Z", "x", None), "正文内容");
        up(&s, &meta_full("b", "/p/ai", Tool::Claude, "2026-06-02T00:00:00Z", "x", None), "正文内容");
        let st = s.stats().unwrap();
        assert_eq!(st.total_body_chars, 8);
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

    /// 工具过滤：只搜 claude / 只搜 codex / 都选则不限制。
    #[test]
    fn search_filters_by_tool() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta_tool("a", "/p/ai", "标题", "2026-06-16", Tool::Claude), "共同关键词 检索目标");
        up(&s, &meta_tool("b", "/p/ai", "标题", "2026-06-15", Tool::Codex), "共同关键词 检索目标");

        // 只选 claude
        let only_claude = vec!["claude".to_string()];
        let r = s.search_filtered("检索目标", None, None, Some(&only_claude), None).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].meta.id, "a");
        // 只选 codex
        let only_codex = vec!["codex".to_string()];
        let r = s.search_filtered("检索目标", None, None, Some(&only_codex), None).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].meta.id, "b");
        // 两个都选 → 不限制
        let both = vec!["claude".to_string(), "codex".to_string()];
        let r = s.search_filtered("检索目标", None, None, Some(&both), None).unwrap();
        assert_eq!(r.len(), 2);
        // None → 不限制
        let r = s.search_filtered("检索目标", None, None, None, None).unwrap();
        assert_eq!(r.len(), 2);
    }

    /// 目录过滤：限定某 cwd。
    #[test]
    fn search_filters_by_cwd() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta("a", "/p/ai", "标题", "2026-06-16"), "共同关键词 检索目标");
        up(&s, &meta("b", "/p/hub", "标题", "2026-06-15"), "共同关键词 检索目标");

        let r = s.search_filtered("检索目标", None, None, None, Some("/p/ai")).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].meta.cwd, "/p/ai");
        // 空字符串 = 不过滤
        let r = s.search_filtered("检索目标", None, None, None, Some("")).unwrap();
        assert_eq!(r.len(), 2);
    }

    /// 组合过滤：tool + cwd + since 同时生效（AND）。
    #[test]
    fn search_combined_filters() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta_tool("a", "/p/ai", "标题", "2026-06-16T00:00:00Z", Tool::Claude), "共同关键词 检索目标");
        up(&s, &meta_tool("b", "/p/ai", "标题", "2026-01-01T00:00:00Z", Tool::Claude), "共同关键词 检索目标");
        up(&s, &meta_tool("c", "/p/hub", "标题", "2026-06-16T00:00:00Z", Tool::Claude), "共同关键词 检索目标");
        up(&s, &meta_tool("d", "/p/ai", "标题", "2026-06-16T00:00:00Z", Tool::Codex), "共同关键词 检索目标");

        // claude + /p/ai + since 2026-03 → 只有 a
        let claude = vec!["claude".to_string()];
        let r = s.search_filtered("检索目标", None, Some("2026-03-01"), Some(&claude), Some("/p/ai")).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].meta.id, "a");
    }

    /// 工具过滤在 LIKE 兜底路径（CJK 2 字）也生效。
    #[test]
    fn search_tool_filter_works_in_like_path() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta_tool("a", "/p/ai", "旅迹", "2026-06-16", Tool::Claude), "x");
        up(&s, &meta_tool("b", "/p/ai", "旅迹", "2026-06-15", Tool::Codex), "y");
        let only_claude = vec!["claude".to_string()];
        let r = s.search_filtered("旅迹", None, None, Some(&only_claude), None).unwrap(); // 2 字 → LIKE
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].meta.id, "a");
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

    // ---- 自定义标题（重命名）----

    #[test]
    fn custom_title_overrides_in_list_sessions() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta("a", "/p/x", "派生标题", "2026-06-16"), "正文");
        s.set_custom_title("/f/a", "我的自定义标题").unwrap();
        let list = s.list_sessions("/p/x").unwrap();
        assert_eq!(list[0].title, "我的自定义标题");
    }

    /// 核心：自定义标题不被增量重索引（再次 upsert 派生标题）覆盖。
    #[test]
    fn custom_title_survives_reupsert() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta("a", "/p/x", "派生甲", "2026-06-16"), "正文");
        s.set_custom_title("/f/a", "自定义").unwrap();
        // 模拟增量重索引：用新派生标题再次 upsert 同一 file_path
        up(&s, &meta("a", "/p/x", "派生乙", "2026-06-17"), "正文改");
        let list = s.list_sessions("/p/x").unwrap();
        assert_eq!(list[0].title, "自定义", "自定义标题应不被重索引覆盖");
    }

    #[test]
    fn clear_custom_title_reverts_to_derived() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta("a", "/p/x", "派生标题", "2026-06-16"), "正文");
        s.set_custom_title("/f/a", "自定义").unwrap();
        s.clear_custom_title("/f/a").unwrap();
        let list = s.list_sessions("/p/x").unwrap();
        assert_eq!(list[0].title, "派生标题");
        assert_eq!(s.derived_title("/f/a").unwrap().as_deref(), Some("派生标题"));
    }

    #[test]
    fn custom_title_overrides_in_search() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta("a", "/p/x", "无关标题", "2026-06-16"), "独特正文关键词检索");
        s.set_custom_title("/f/a", "自定义显示名").unwrap();
        let hits = s.search("独特正文关键词", None, None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].meta.title, "自定义显示名");
    }

    #[test]
    fn delete_paths_cleans_custom_titles() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta("a", "/p/x", "t", "2026-06-16"), "正文");
        s.set_custom_title("/f/a", "自定义").unwrap();
        s.delete_paths(&["/f/a".to_string()]).unwrap();
        // 同 file_path 会话再次出现（重扫）→ 旧自定义标题不应残留
        up(&s, &meta("a", "/p/x", "新派生", "2026-06-17"), "正文");
        let list = s.list_sessions("/p/x").unwrap();
        assert_eq!(list[0].title, "新派生", "删除后自定义标题不应残留");
    }

    #[test]
    fn delete_cwd_cleans_custom_titles() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta("a", "/p/x", "t", "2026-06-16"), "正文");
        s.set_custom_title("/f/a", "自定义").unwrap();
        s.delete_cwd("/p/x").unwrap();
        up(&s, &meta("a", "/p/x", "新派生", "2026-06-17"), "正文");
        let list = s.list_sessions("/p/x").unwrap();
        assert_eq!(list[0].title, "新派生", "删除目录后自定义标题不应残留");
    }

    // ---- fork 谱系（has_children / session_by_path）----

    /// 带 forked_from 的会话构造（fork 关系测试用）。
    fn meta_fork(id: &str, cwd: &str, parent: Option<&str>, updated: &str) -> SessionMeta {
        SessionMeta {
            id: id.into(), tool: Tool::Codex, cwd: cwd.into(),
            file_path: format!("/f/{}", id), title: format!("会话{}", id),
            started_at: updated.into(), updated_at: updated.into(),
            message_count: 1, forked_from: parent.map(|x| x.into()),
            resume_command: Tool::Codex.resume_command(id), has_children: false,
            favorited: false, collection_ids: Vec::new(),
        }
    }

    /// list_sessions 计算 has_children：被 fork 的父=true，叶子=false。
    #[test]
    fn list_sessions_computes_has_children() {
        let s = Store::open_in_memory().unwrap();
        s.upsert(&meta_fork("parent", "/p/ai", None, "2026-06-16"), "x", "x", "", 0).unwrap();
        s.upsert(&meta_fork("child", "/p/ai", Some("parent"), "2026-06-15"), "y", "y", "", 0).unwrap();

        let list = s.list_sessions("/p/ai").unwrap();
        let parent = list.iter().find(|m| m.id == "parent").unwrap();
        let child = list.iter().find(|m| m.id == "child").unwrap();
        assert!(parent.has_children, "被 fork 的父应 has_children=true");
        assert!(!child.has_children, "叶子会话 has_children=false");
    }

    /// session_by_path 命中与未命中。
    #[test]
    fn session_by_path_finds_and_misses() {
        let s = Store::open_in_memory().unwrap();
        s.upsert(&meta_fork("a", "/p/ai", None, "2026-06-16"), "x", "x", "", 0).unwrap();
        let got = s.session_by_path("/f/a").unwrap();
        assert_eq!(got.unwrap().id, "a");
        assert!(s.session_by_path("/f/none").unwrap().is_none());
    }

    /// AI 配置读写：默认关闭、有默认地址/模型；写后读回。
    #[test]
    fn ai_config_read_write() {
        let s = Store::open_in_memory().unwrap();
        let c = s.ai_config().unwrap();
        assert!(!c.enabled, "默认关闭");
        assert!(c.api_key.is_empty(), "默认无 key");
        assert_eq!(c.model, "claude-opus-4-8");

        let new = crate::ai::AiConfig {
            enabled: true,
            base_url: "https://x/v1/messages".into(),
            api_key: "sk-secret".into(),
            model: "claude-opus-4-8".into(),
        };
        s.set_ai_config(&new).unwrap();
        let got = s.ai_config().unwrap();
        assert!(got.enabled);
        assert_eq!(got.api_key, "sk-secret");
        assert_eq!(got.base_url, "https://x/v1/messages");
    }

    /// AI 标题缓存读写 + 三层优先级（用户 > AI > 启发式）。
    #[test]
    fn ai_title_cache_and_priority() {
        let s = Store::open_in_memory().unwrap();
        s.upsert(&meta_fork("a", "/p/ai", None, "2026-06-16"), "x", "x", "", 0).unwrap();
        // 仅启发式
        assert_eq!(s.list_sessions("/p/ai").unwrap()[0].title, "会话a");
        // 加 AI 标题 → 覆盖启发式
        s.set_ai_title("/f/a", "AI概括的标题").unwrap();
        assert_eq!(s.ai_title("/f/a").unwrap().as_deref(), Some("AI概括的标题"));
        assert_eq!(s.list_sessions("/p/ai").unwrap()[0].title, "AI概括的标题");
        // 加用户自定义 → 覆盖 AI
        s.set_custom_title("/f/a", "用户标题").unwrap();
        assert_eq!(s.list_sessions("/p/ai").unwrap()[0].title, "用户标题", "用户 > AI");
    }

    /// 删除会话时一并清 ai_titles（不残留）。
    #[test]
    fn delete_cleans_ai_titles() {
        let s = Store::open_in_memory().unwrap();
        s.upsert(&meta_fork("a", "/p/ai", None, "2026-06-16"), "x", "x", "", 0).unwrap();
        s.set_ai_title("/f/a", "AI标题").unwrap();
        s.delete_paths(&["/f/a".to_string()]).unwrap();
        assert!(s.ai_title("/f/a").unwrap().is_none(), "删除后 ai_title 应清除");
    }

    /// 标题逻辑版本号读写：缺省 0，写后可读回。
    #[test]
    fn title_logic_version_read_write() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.title_logic_version().unwrap(), 0, "缺省应为 0");
        s.set_title_logic_version(3).unwrap();
        assert_eq!(s.title_logic_version().unwrap(), 3);
        s.set_title_logic_version(5).unwrap(); // 覆盖
        assert_eq!(s.title_logic_version().unwrap(), 5);
    }

    // ---- AI 摘要缓存 ----

    /// 摘要缓存读写：写后可反序列化读回，缺省 None。
    #[test]
    fn ai_summary_read_write() {
        use crate::ai::AiSummary;
        let s = Store::open_in_memory().unwrap();
        assert!(s.ai_summary("/f/a").unwrap().is_none(), "缺省应为 None");

        let sum = AiSummary {
            gist: "修复登录态".into(),
            conclusions: vec!["改用Cookie".into(), "加静默刷新".into()],
            files: vec!["src/auth.ts".into()],
        };
        s.set_ai_summary("/f/a", &sum, "claude-opus-4-8", "2026-06-19").unwrap();
        let got = s.ai_summary("/f/a").unwrap().unwrap();
        assert_eq!(got, sum);
    }

    /// 摘要缓存覆盖写（INSERT OR REPLACE）。
    #[test]
    fn ai_summary_replace() {
        use crate::ai::AiSummary;
        let s = Store::open_in_memory().unwrap();
        let a = AiSummary { gist: "旧".into(), conclusions: vec![], files: vec![] };
        let b = AiSummary { gist: "新".into(), conclusions: vec![], files: vec![] };
        s.set_ai_summary("/f/a", &a, "m", "t").unwrap();
        s.set_ai_summary("/f/a", &b, "m", "t").unwrap();
        assert_eq!(s.ai_summary("/f/a").unwrap().unwrap().gist, "新");
    }

    /// 删除会话时一并清 ai_summaries（不残留）。
    #[test]
    fn delete_cleans_ai_summaries() {
        use crate::ai::AiSummary;
        let s = Store::open_in_memory().unwrap();
        s.upsert(&meta_fork("a", "/p/ai", None, "2026-06-16"), "x", "x", "", 0).unwrap();
        let sum = AiSummary { gist: "摘要".into(), conclusions: vec![], files: vec![] };
        s.set_ai_summary("/f/a", &sum, "m", "t").unwrap();
        s.delete_paths(&["/f/a".to_string()]).unwrap();
        assert!(s.ai_summary("/f/a").unwrap().is_none(), "删除后 ai_summary 应清除");
    }

    /// delete_cwd 也清 ai_summaries。
    #[test]
    fn delete_cwd_cleans_ai_summaries() {
        use crate::ai::AiSummary;
        let s = Store::open_in_memory().unwrap();
        s.upsert(&meta_fork("a", "/p/ai", None, "2026-06-16"), "x", "x", "", 0).unwrap();
        let sum = AiSummary { gist: "摘要".into(), conclusions: vec![], files: vec![] };
        s.set_ai_summary("/f/a", &sum, "m", "t").unwrap();
        s.delete_cwd("/p/ai").unwrap();
        assert!(s.ai_summary("/f/a").unwrap().is_none(), "delete_cwd 后 ai_summary 应清除");
    }

    // ============ 收藏 + 分类（Collections）测试 ============

    #[test]
    fn collection_crud() {
        let s = Store::open_in_memory().unwrap();
        // 创建 → 返回带 id 的分类，sort 自增。
        let c1 = s.create_collection("踩坑记录", "coral").unwrap();
        let c2 = s.create_collection("重构方案", "teal").unwrap();
        assert_eq!(c1.name, "踩坑记录");
        assert_eq!(c1.color, "coral");
        assert!(c2.sort > c1.sort, "新分类 sort 应递增");

        // 列出（按 sort 升序）。
        let list = s.list_collections().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, c1.id);
        assert_eq!(list[1].id, c2.id);

        // 改名 + 改色。
        s.rename_collection(&c1.id, "踩坑", "amber").unwrap();
        let list = s.list_collections().unwrap();
        assert_eq!(list[0].name, "踩坑");
        assert_eq!(list[0].color, "amber");

        // 删除。
        s.delete_collection(&c2.id).unwrap();
        assert_eq!(s.list_collections().unwrap().len(), 1);
    }

    #[test]
    fn reorder_collections_sets_new_order() {
        let s = Store::open_in_memory().unwrap();
        let a = s.create_collection("A", "slate").unwrap();
        let b = s.create_collection("B", "slate").unwrap();
        let c = s.create_collection("C", "slate").unwrap();
        // 重排为 c, a, b。
        s.reorder_collections(&[c.id.clone(), a.id.clone(), b.id.clone()]).unwrap();
        let list = s.list_collections().unwrap();
        assert_eq!(list.iter().map(|x| x.id.clone()).collect::<Vec<_>>(), vec![c.id, a.id, b.id]);
    }

    #[test]
    fn set_favorite_toggles_and_overlays() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta("a", "/p/ai", "会话A", "2026-06-16"), "正文");
        // 收藏（未归类）。
        s.set_favorite("/f/a", &[], true).unwrap();
        let sessions = s.list_sessions("/p/ai").unwrap();
        assert!(sessions[0].favorited, "收藏后 list_sessions 应 overlay favorited=true");
        // 取消收藏。
        s.set_favorite("/f/a", &[], false).unwrap();
        let sessions = s.list_sessions("/p/ai").unwrap();
        assert!(!sessions[0].favorited, "取消收藏后 favorited=false");
    }

    #[test]
    fn set_favorite_with_collections_is_idempotent() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta("a", "/p/ai", "会话A", "2026-06-16"), "正文");
        let c1 = s.create_collection("踩坑", "coral").unwrap();
        let c2 = s.create_collection("灵感", "amber").unwrap();
        // 收藏并归入两个分类。
        s.set_favorite("/f/a", &[c1.id.clone(), c2.id.clone()], true).unwrap();
        // 幂等：重复设置同样的分类不应报错或翻倍。
        s.set_favorite("/f/a", &[c1.id.clone(), c2.id.clone()], true).unwrap();
        // 各分类计数应为 1。
        let list = s.list_collections().unwrap();
        let cc1 = list.iter().find(|c| c.id == c1.id).unwrap();
        assert_eq!(cc1.count, 1, "分类 count 应为 1（幂等）");
        // 改为仅归入 c1（覆盖语义：从 c2 移除）。
        s.set_favorite("/f/a", &[c1.id.clone()], true).unwrap();
        let list = s.list_collections().unwrap();
        assert_eq!(list.iter().find(|c| c.id == c1.id).unwrap().count, 1);
        assert_eq!(list.iter().find(|c| c.id == c2.id).unwrap().count, 0, "改归类后 c2 应不再含此会话");
    }

    #[test]
    fn list_favorites_filters_by_collection_and_query() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta("a", "/p/ai", "数据库泄漏排查", "2026-06-16"), "连接池正文");
        up(&s, &meta("b", "/p/ai", "重构索引器", "2026-06-15"), "rayon 并行正文");
        up(&s, &meta("c", "/p/ai", "随便聊聊", "2026-06-14"), "闲聊正文");
        let c1 = s.create_collection("踩坑", "coral").unwrap();
        s.set_favorite("/f/a", &[c1.id.clone()], true).unwrap();
        s.set_favorite("/f/b", &[], true).unwrap(); // 收藏但未归类
        // c 不收藏。

        // 全部收藏：a + b（按 updated_at 降序）。
        let all = s.list_favorites(None, None).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].file_path, "/f/a");
        assert!(all.iter().all(|m| m.favorited), "list_favorites 结果都应 favorited=true");

        // 按分类筛：仅 c1 → a。
        let in_c1 = s.list_favorites(Some(&c1.id), None).unwrap();
        assert_eq!(in_c1.len(), 1);
        assert_eq!(in_c1[0].file_path, "/f/a");
        assert!(in_c1[0].collection_ids.contains(&c1.id), "结果应带所属 collection_ids");

        // 收藏视图搜索：在全部收藏中搜「索引」→ 仅 b。
        let hit = s.list_favorites(None, Some("索引")).unwrap();
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].file_path, "/f/b");

        // 搜索 + 分类组合：在 c1 中搜「索引」→ 空（b 不在 c1）。
        let none = s.list_favorites(Some(&c1.id), Some("索引")).unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn delete_collection_demotes_favorites_not_drops() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta("a", "/p/ai", "会话A", "2026-06-16"), "正文");
        let c1 = s.create_collection("踩坑", "coral").unwrap();
        s.set_favorite("/f/a", &[c1.id.clone()], true).unwrap();
        // 删除分类 → 会话仍被收藏（降级为未分类），不丢收藏。
        s.delete_collection(&c1.id).unwrap();
        let all = s.list_favorites(None, None).unwrap();
        assert_eq!(all.len(), 1, "删分类后会话仍收藏");
        assert!(all[0].collection_ids.is_empty(), "删分类后该会话降级为未分类");
    }

    #[test]
    fn delete_collection_keeps_multi_collection_session_classified() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta("a", "/p/ai", "会话A", "2026-06-16"), "正文");
        let c1 = s.create_collection("踩坑", "coral").unwrap();
        let c2 = s.create_collection("灵感", "amber").unwrap();
        // 会话同属两个分类。
        s.set_favorite("/f/a", &[c1.id.clone(), c2.id.clone()], true).unwrap();
        // 删除 c1：会话仍属 c2，不应留下冗余 '' 哨兵行。
        s.delete_collection(&c1.id).unwrap();
        let in_c2 = s.list_favorites(Some(&c2.id), None).unwrap();
        assert_eq!(in_c2.len(), 1, "删 c1 后会话仍在 c2");
        assert_eq!(in_c2[0].collection_ids, vec![c2.id.clone()], "仍仅归类 c2,无未分类残留");
        // favorites 表中不应有该会话的 '' 哨兵行（仅剩 c2 一行）。
        let sentinel_cnt: i64 = s
            .conn
            .query_row(
                "SELECT COUNT(*) FROM favorites WHERE file_path = '/f/a' AND collection_id = ''",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sentinel_cnt, 0, "多分类会话删其一分类后不应留下冗余未分类哨兵");
    }

    #[test]
    fn list_favorites_escapes_backslash_in_query() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta("a", "/p/ai", "路径 C:\\Users\\leo", "2026-06-16"), "windows 路径正文");
        up(&s, &meta("b", "/p/ai", "无关会话", "2026-06-15"), "其它正文");
        s.set_favorite("/f/a", &[], true).unwrap();
        s.set_favorite("/f/b", &[], true).unwrap();
        // 含反斜杠的查询应安全匹配（不因 ESCAPE 误判而报错或错配）。
        let hit = s.list_favorites(None, Some("C:\\Users")).unwrap();
        assert_eq!(hit.len(), 1, "含反斜杠查询应正确命中标题");
        assert_eq!(hit[0].file_path, "/f/a");
    }

    #[test]
    fn delete_paths_cleans_favorites() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta("a", "/p/ai", "会话A", "2026-06-16"), "正文");
        let c1 = s.create_collection("踩坑", "coral").unwrap();
        s.set_favorite("/f/a", &[c1.id.clone()], true).unwrap();
        s.delete_paths(&["/f/a".to_string()]).unwrap();
        assert!(s.list_favorites(None, None).unwrap().is_empty(), "删除会话应清理 favorites");
        assert_eq!(s.list_collections().unwrap()[0].count, 0, "删除会话后分类 count 归零");
    }

    #[test]
    fn delete_cwd_cleans_favorites() {
        let s = Store::open_in_memory().unwrap();
        up(&s, &meta("a", "/p/ai", "会话A", "2026-06-16"), "正文");
        s.set_favorite("/f/a", &[], true).unwrap();
        s.delete_cwd("/p/ai").unwrap();
        assert!(s.list_favorites(None, None).unwrap().is_empty(), "delete_cwd 应清理 favorites");
    }
}
