//! 遍历 Claude / Codex 会话根目录，产出待解析文件清单。
use crate::models::Tool;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct ScanItem {
    pub tool: Tool,
    pub path: PathBuf,
    /// 文件修改时间（自 Unix 纪元的毫秒）；增量索引据此判断是否需要重新解析。
    pub mtime: i64,
}

/// 读取文件修改时间（毫秒）；任何失败回退 0（视为"未知"，会被当作需重建）。
fn file_mtime_millis(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 默认的 Claude 根目录 `~/.claude/projects`。
pub fn default_claude_root() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join("projects"))
}

/// 默认的 Codex 根目录 `~/.codex/sessions`。
pub fn default_codex_root() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".codex").join("sessions"))
}

fn collect_jsonl(root: &Path, tool: Tool, out: &mut Vec<ScanItem>) {
    if !root.exists() {
        return;
    }
    for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if p.is_file() && p.extension().map(|e| e == "jsonl").unwrap_or(false) {
            out.push(ScanItem {
                tool,
                mtime: file_mtime_millis(p),
                path: p.to_path_buf(),
            });
        }
    }
}

/// 扫描两个根目录，返回所有会话文件（带工具标记）。
pub fn scan_files(claude_root: &Path, codex_root: &Path) -> Vec<ScanItem> {
    let mut out = Vec::new();
    collect_jsonl(claude_root, Tool::Claude, &mut out);
    collect_jsonl(codex_root, Tool::Codex, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn finds_jsonl_in_both_roots() {
        let claude = tempfile::tempdir().unwrap();
        let codex = tempfile::tempdir().unwrap();

        // Claude: projects/<dir>/x.jsonl
        let cdir = claude.path().join("-Users-leo-proj");
        std::fs::create_dir_all(&cdir).unwrap();
        std::fs::File::create(cdir.join("a.jsonl")).unwrap().write_all(b"{}").unwrap();
        std::fs::File::create(cdir.join("ignore.txt")).unwrap().write_all(b"x").unwrap();

        // Codex: 2026/03/16/rollout.jsonl
        let xdir = codex.path().join("2026").join("03").join("16");
        std::fs::create_dir_all(&xdir).unwrap();
        std::fs::File::create(xdir.join("rollout-1.jsonl")).unwrap().write_all(b"{}").unwrap();

        let items = scan_files(claude.path(), codex.path());
        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|i| i.tool == Tool::Claude));
        assert!(items.iter().any(|i| i.tool == Tool::Codex));
        assert!(items.iter().all(|i| i.mtime > 0), "应采集到文件 mtime");
    }

    #[test]
    fn missing_root_is_ok() {
        let items = scan_files(Path::new("/no/such/claude"), Path::new("/no/such/codex"));
        assert!(items.is_empty());
    }
}
