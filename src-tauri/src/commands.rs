//! Tauri 命令层：前端通过 invoke 调用。
use crate::indexer::{build_index, ScanSummary};
use crate::models::{Message, Project, SearchHit, SessionMeta, Tool};
use crate::parsers::{claude, codex};
use crate::scanner::{default_claude_root, default_codex_root};
use crate::store::Store;
use crate::terminal;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::State;

pub struct AppState {
    pub store: Mutex<Store>,
    pub claude_root: PathBuf,
    pub codex_root: PathBuf,
}

impl AppState {
    pub fn new() -> Result<AppState, String> {
        // 磁盘 SQLite，存放于应用数据目录，承载全文索引。
        let db_path = dirs::data_dir()
            .map(|d| d.join("de.aigy.backtrack"))
            .ok_or("无法定位数据目录")?;
        std::fs::create_dir_all(&db_path).map_err(|e| e.to_string())?;
        let store = Store::open(&db_path.join("index.db")).map_err(|e| e.to_string())?;
        Ok(AppState {
            store: Mutex::new(store),
            claude_root: default_claude_root().unwrap_or_default(),
            codex_root: default_codex_root().unwrap_or_default(),
        })
    }
}

#[tauri::command]
pub fn scan(state: State<'_, AppState>) -> Result<ScanSummary, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let summary = build_index(&store, &state.claude_root, &state.codex_root);
    Ok(summary)
}

#[tauri::command]
pub fn list_projects(state: State<'_, AppState>) -> Result<Vec<Project>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.list_projects().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_sessions(state: State<'_, AppState>, cwd: String) -> Result<Vec<SessionMeta>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.list_sessions(&cwd).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn search(
    state: State<'_, AppState>,
    query: String,
    role: Option<String>,
    since: Option<String>,
) -> Result<Vec<SearchHit>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(vec![]);
    }
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store
        .search(q, role.as_deref(), since.as_deref())
        .map_err(|e| e.to_string())
}

/// 按需解析单个会话文件的完整对话（懒加载正文）。
#[tauri::command]
pub fn get_transcript(file_path: String, tool: String) -> Result<Vec<Message>, String> {
    let path = PathBuf::from(&file_path);
    let parsed = match Tool::from_str(&tool) {
        Some(Tool::Claude) => claude::parse_claude(&path),
        Some(Tool::Codex) => codex::parse_codex(&path),
        None => return Err(format!("未知工具类型: {}", tool)),
    };
    parsed
        .map(|(_, msgs)| msgs)
        .ok_or_else(|| format!("无法解析会话文件: {}", file_path))
}

#[tauri::command]
pub fn resume_in_terminal(cwd: String, command: String, terminal: String) -> Result<(), String> {
    terminal::resume_in_terminal(&cwd, &command, &terminal)
}

/// 删除文件后，把变空的 Claude 项目目录也移到废纸篓。
fn trash_empty_claude_dirs(paths: &[String]) {
    let mut dirs: std::collections::HashSet<std::path::PathBuf> = std::collections::HashSet::new();
    for p in paths {
        if let Some(parent) = Path::new(p).parent() {
            dirs.insert(parent.to_path_buf());
        }
    }
    for d in dirs {
        let is_claude_proj = d.to_string_lossy().contains("/.claude/projects/");
        let empty = std::fs::read_dir(&d).map(|mut r| r.next().is_none()).unwrap_or(false);
        if is_claude_proj && empty {
            let _ = trash::delete(&d);
        }
    }
}

/// 删除某目录：把其全部会话文件移到废纸篓，清理残留空目录，从索引删除。返回删除条数。
#[tauri::command]
pub fn delete_project(state: State<'_, AppState>, cwd: String) -> Result<usize, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let paths = store.paths_for_cwd(&cwd).map_err(|e| e.to_string())?;
    if !paths.is_empty() {
        trash::delete_all(&paths).map_err(|e| format!("移到废纸篓失败: {}", e))?;
        trash_empty_claude_dirs(&paths);
    }
    let n = store.delete_cwd(&cwd).map_err(|e| e.to_string())?;
    Ok(n)
}

/// 删除若干会话：把指定 jsonl 文件移到废纸篓，从索引删除。返回删除条数。
#[tauri::command]
pub fn delete_sessions(state: State<'_, AppState>, paths: Vec<String>) -> Result<usize, String> {
    if paths.is_empty() {
        return Ok(0);
    }
    let store = state.store.lock().map_err(|e| e.to_string())?;
    trash::delete_all(&paths).map_err(|e| format!("移到废纸篓失败: {}", e))?;
    trash_empty_claude_dirs(&paths);
    let n = store.delete_paths(&paths).map_err(|e| e.to_string())?;
    Ok(n)
}

#[tauri::command]
pub fn hide_project(state: State<'_, AppState>, cwd: String) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.hide(&cwd).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn unhide_project(state: State<'_, AppState>, cwd: String) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.unhide(&cwd).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_hidden(state: State<'_, AppState>) -> Result<Vec<Project>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.list_hidden().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_starred(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.list_starred().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_star(state: State<'_, AppState>, cwd: String, starred: bool) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.set_star(&cwd, starred).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_starred_all(state: State<'_, AppState>, cwds: Vec<String>) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.set_starred_all(&cwds).map_err(|e| e.to_string())
}

/// 在 Finder 中打开目录(reveal=false) 或定位文件(reveal=true)。
#[tauri::command]
pub fn reveal_in_finder(path: String, reveal: bool) -> Result<(), String> {
    let mut cmd = std::process::Command::new("open");
    if reveal {
        cmd.arg("-R");
    }
    cmd.arg(&path);
    cmd.spawn().map_err(|e| format!("打开 Finder 失败: {}", e))?;
    Ok(())
}
