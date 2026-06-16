//! SQLite 元数据缓存 + 子串搜索（CJK 友好，用 LIKE）。
use crate::models::{display_name_for, Project, SessionMeta, Tool};
use rusqlite::{params, Connection};

/// 单条会话正文最多缓存的字符数（足够覆盖超长会话的全部对话，
/// 又能挡住病态超大文件；磁盘 SQLite 承载）。
const BODY_CAP: usize = 2_000_000;

pub struct Store {
    pub conn: Connection,
}

impl Store {
    pub fn open_in_memory() -> rusqlite::Result<Store> {
        let conn = Connection::open_in_memory()?;
        let s = Store { conn };
        s.init_schema()?;
        Ok(s)
    }

    pub fn open(path: &std::path::Path) -> rusqlite::Result<Store> {
        let conn = Connection::open(path)?;
        let s = Store { conn };
        s.init_schema()?;
        Ok(s)
    }

    fn init_schema(&self) -> rusqlite::Result<()> {
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
                body        TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_sessions_cwd ON sessions(cwd);
            CREATE INDEX IF NOT EXISTS idx_sessions_updated ON sessions(updated_at);
            CREATE TABLE IF NOT EXISTS hidden (cwd TEXT PRIMARY KEY);
            CREATE TABLE IF NOT EXISTS starred (cwd TEXT PRIMARY KEY);",
        )
    }

    pub fn clear(&self) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM sessions", [])?;
        Ok(())
    }

    /// 写入（或覆盖）一条会话。`body` 用于搜索，自动截断。
    pub fn upsert(&self, m: &SessionMeta, body: &str) -> rusqlite::Result<()> {
        let capped: String = body.chars().take(BODY_CAP).collect();
        self.conn.execute(
            "INSERT OR REPLACE INTO sessions
             (id, tool, cwd, file_path, title, started_at, updated_at, msg_count, forked_from, body)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                m.id, m.tool.as_str(), m.cwd, m.file_path, m.title,
                m.started_at, m.updated_at, m.message_count as i64, m.forked_from, capped
            ],
        )?;
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

    /// 全局子串搜索（标题 + 正文），按最近活动降序。
    pub fn search(&self, query: &str) -> rusqlite::Result<Vec<SessionMeta>> {
        let like = format!("%{}%", query);
        let mut stmt = self.conn.prepare(
            "SELECT * FROM sessions WHERE title LIKE ?1 OR body LIKE ?1
             ORDER BY updated_at DESC LIMIT 200",
        )?;
        let rows = stmt.query_map(params![like], Self::row_to_meta)?;
        rows.collect()
    }

    pub fn count(&self) -> rusqlite::Result<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))?;
        Ok(n as usize)
    }
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

    #[test]
    fn aggregates_projects_and_filters_sessions() {
        let s = Store::open_in_memory().unwrap();
        s.upsert(&meta("a", "/p/ai", "旅迹原型", "2026-06-16"), "建 stitch 项目 旅迹").unwrap();
        s.upsert(&meta("b", "/p/ai", "typora 主题", "2026-06-15"), "改造 juejin 主题").unwrap();
        s.upsert(&meta("c", "/p/hub", "登录 bug", "2026-06-14"), "oauth 回调").unwrap();

        let projs = s.list_projects().unwrap();
        assert_eq!(projs.len(), 2);
        assert_eq!(projs[0].path, "/p/ai"); // 会话多的排前
        assert_eq!(projs[0].session_count, 2);

        let ai = s.list_sessions("/p/ai").unwrap();
        assert_eq!(ai.len(), 2);
        assert_eq!(ai[0].id, "a"); // 最近的排前
    }

    #[test]
    fn search_matches_cjk_substring() {
        let s = Store::open_in_memory().unwrap();
        s.upsert(&meta("a", "/p/ai", "旅迹小程序原型", "2026-06-16"), "情侣旅行回忆地图").unwrap();
        s.upsert(&meta("b", "/p/ai", "typora 主题", "2026-06-15"), "改造主题").unwrap();

        let hits = s.search("旅迹").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "a");

        let body_hit = s.search("回忆地图").unwrap();
        assert_eq!(body_hit.len(), 1);
    }

    #[test]
    fn hide_excludes_from_projects_and_lists_separately() {
        let s = Store::open_in_memory().unwrap();
        s.upsert(&meta("a", "/p/keep", "保留", "2026-06-16"), "x").unwrap();
        s.upsert(&meta("b", "/p/junk", "垃圾", "2026-06-15"), "y").unwrap();

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
        s.upsert(&meta("a", "/p/x", "t1", "2026-06-16"), "x").unwrap();
        s.upsert(&meta("b", "/p/x", "t2", "2026-06-15"), "y").unwrap();
        s.upsert(&meta("c", "/p/other", "t3", "2026-06-14"), "z").unwrap();

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
        s.upsert(&meta("a", "/p/x", "t1", "2026-06-16"), "x").unwrap();
        s.upsert(&meta("b", "/p/x", "t2", "2026-06-15"), "y").unwrap();
        s.upsert(&meta("c", "/p/x", "t3", "2026-06-14"), "z").unwrap();

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
        s.upsert(&meta("a", "/p", "t", "2026-06-16"), "x").unwrap();
        s.upsert(&meta("a", "/p", "t2", "2026-06-17"), "x").unwrap();
        assert_eq!(s.count().unwrap(), 1);
    }
}
