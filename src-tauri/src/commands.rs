//! Tauri 命令层：前端通过 invoke 调用。
use crate::ai::{self, AiConfig};
use crate::export::{self, ExportFormat};
use crate::fork::{build_fork_tree, ForkNode};
use crate::indexer::{build_index, ScanSummary};
use crate::models::{Collection, Message, Project, SearchHit, SessionMeta, StatsDto, Tool};
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

/// 列出某父会话的全部子代理（折叠区用）。
#[tauri::command]
pub fn list_subagents(
    state: State<'_, AppState>,
    parent_id: String,
) -> Result<Vec<crate::models::SubagentInfo>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.list_subagents(&parent_id).map_err(|e| e.to_string())
}

/// 全局使用统计（统计面板用，只读聚合，不触网）。
#[tauri::command]
pub fn stats(state: State<'_, AppState>) -> Result<StatsDto, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.stats().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn search(
    state: State<'_, AppState>,
    query: String,
    role: Option<String>,
    since: Option<String>,
    tools: Option<Vec<String>>,
    cwd: Option<String>,
) -> Result<Vec<SearchHit>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(vec![]);
    }
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store
        .search_filtered(
            q,
            role.as_deref(),
            since.as_deref(),
            tools.as_deref(),
            cwd.as_deref(),
        )
        .map_err(|e| e.to_string())
}

/// 按需解析单个会话文件的完整对话（懒加载正文）。
#[tauri::command]
pub fn get_transcript(file_path: String, tool: String) -> Result<Vec<Message>, String> {
    parse_transcript(&file_path, &tool).map(|(_, msgs)| msgs)
}

/// 解析会话文件 → (SessionMeta, Vec<Message>)。get_transcript 与 export_session 共用。
fn parse_transcript(file_path: &str, tool: &str) -> Result<(SessionMeta, Vec<Message>), String> {
    let path = PathBuf::from(file_path);
    let parsed = match Tool::from_str(tool) {
        // 子代理文件 path 含 subagents 段时自动带上 parent_id（影响导出标题派生）。
        Some(Tool::Claude) => claude::parse_claude(&path, crate::scanner::parent_id_from_path(&path)),
        Some(Tool::Codex) => codex::parse_codex(&path),
        None => return Err(format!("未知工具类型: {}", tool)),
    };
    parsed.ok_or_else(|| format!("无法解析会话文件: {}", file_path))
}

/// 导出单会话为 Markdown/HTML。弹「另存为」对话框；用户取消返回 Ok(None)。
/// 成功返回 Ok(Some(保存路径))。仅读取原始 jsonl，绝不修改。
/// title 为前端已 override 的自定义/派生标题，用于文档标题与默认文件名。
///
/// 注意：声明为 async，使其不在主线程执行；并用**非阻塞**的 `save_file` 回调，
/// 避免阻塞 UI 线程导致整个应用卡死（blocking_save_file 不可用于主线程）。
#[tauri::command]
pub async fn export_session(
    app: tauri::AppHandle,
    file_path: String,
    tool: String,
    title: String,
    format: String,
    include_tools: bool,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let fmt = ExportFormat::from_str(&format).ok_or_else(|| format!("未知导出格式: {}", format))?;
    let (mut meta, messages) = parse_transcript(&file_path, &tool)?;
    // 用前端传入的标题（可能是用户自定义标题）覆盖派生标题。
    if !title.trim().is_empty() {
        meta.title = title;
    }

    let content = export::render(&meta, &messages, fmt, include_tools);
    let default_name = export::safe_file_name(&meta.title, fmt.ext());

    // 非阻塞「另存为」：通过回调拿到用户选择，channel 异步等待，不占用主线程。
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog()
        .file()
        .set_file_name(&default_name)
        .save_file(move |path| {
            let _ = tx.send(path);
        });
    let chosen = rx.recv().map_err(|e| format!("对话框通道错误: {}", e))?;

    let Some(file_path) = chosen else {
        return Ok(None); // 用户取消，静默。
    };
    let path = file_path
        .into_path()
        .map_err(|e| format!("无效的保存路径: {}", e))?;

    std::fs::write(&path, content).map_err(|e| format!("写入文件失败: {}", e))?;
    Ok(Some(path.to_string_lossy().to_string()))
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

// ============ 收藏 + 分类（Collections） ============

#[tauri::command]
pub fn list_collections(state: State<'_, AppState>) -> Result<Vec<Collection>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.list_collections().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_collection(
    state: State<'_, AppState>,
    name: String,
    color: String,
) -> Result<Collection, String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("分类名称不能为空".into());
    }
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.create_collection(n, &color).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn rename_collection(
    state: State<'_, AppState>,
    id: String,
    name: String,
    color: String,
) -> Result<(), String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("分类名称不能为空".into());
    }
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.rename_collection(&id, n, &color).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_collection(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.delete_collection(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn reorder_collections(state: State<'_, AppState>, ids: Vec<String>) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.reorder_collections(&ids).map_err(|e| e.to_string())
}

/// 收藏 / 取消收藏一个会话，并设置所属分类（覆盖语义）。
/// on=true 且 collection_ids 为空 = 仅收藏不归类；on=false = 取消收藏。
#[tauri::command]
pub fn set_favorite(
    state: State<'_, AppState>,
    file_path: String,
    collection_ids: Vec<String>,
    on: bool,
) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store
        .set_favorite(&file_path, &collection_ids, on)
        .map_err(|e| e.to_string())
}

/// 收藏视图数据：collection_id=None 取全部收藏；query 非空时叠加搜索。
#[tauri::command]
pub fn list_favorites(
    state: State<'_, AppState>,
    collection_id: Option<String>,
    query: Option<String>,
) -> Result<Vec<SessionMeta>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store
        .list_favorites(collection_id.as_deref(), query.as_deref())
        .map_err(|e| e.to_string())
}

/// 重命名会话标题：写入自定义标题（独立持久化，读时 override，不被增量重索引覆盖，绝不改原始 jsonl）。
/// 标题去首尾空白；为空则清除自定义、恢复派生标题。返回生效后的标题。
#[tauri::command]
pub fn rename_session(
    state: State<'_, AppState>,
    file_path: String,
    title: String,
) -> Result<String, String> {
    let t = title.trim();
    let store = state.store.lock().map_err(|e| e.to_string())?;
    if t.is_empty() {
        store.clear_custom_title(&file_path).map_err(|e| e.to_string())?;
        Ok(store
            .derived_title(&file_path)
            .map_err(|e| e.to_string())?
            .unwrap_or_default())
    } else {
        store.set_custom_title(&file_path, t).map_err(|e| e.to_string())?;
        Ok(t.to_string())
    }
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

/// 返回当前会话所属的 fork 谱系树（以链顶为根）。
/// 取目标会话 cwd 下全部会话，在内存构树（含缺父占位、防环）。
/// 孤立会话返回仅含自身的单节点树。只读，绝不修改原始数据。
#[tauri::command]
pub fn fork_tree(state: State<'_, AppState>, file_path: String) -> Result<ForkNode, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let current = store
        .session_by_path(&file_path)
        .map_err(|e| e.to_string())?
        .ok_or("会话不存在")?;
    let sessions = store.list_sessions(&current.cwd).map_err(|e| e.to_string())?;
    Ok(build_fork_tree(&sessions, &file_path))
}

/// 前端读 AI 配置：key 脱敏（仅返回是否已配置 + 掩码），不回传明文。
#[derive(serde::Serialize)]
pub struct AiConfigDto {
    pub enabled: bool,
    pub base_url: String,
    pub model: String,
    pub has_key: bool,
}

/// 读取 AI 配置（key 脱敏，不回传明文）。
#[tauri::command]
pub fn get_ai_config(state: State<'_, AppState>) -> Result<AiConfigDto, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let c = store.ai_config().map_err(|e| e.to_string())?;
    Ok(AiConfigDto {
        enabled: c.enabled,
        base_url: c.base_url,
        model: c.model,
        has_key: !c.api_key.trim().is_empty(),
    })
}

/// 写入 AI 配置。api_key 为空字符串时**保留原有 key**（前端不重填即不覆盖）。
#[tauri::command]
pub fn set_ai_config(
    state: State<'_, AppState>,
    enabled: bool,
    base_url: String,
    api_key: String,
    model: String,
) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let existing = store.ai_config().map_err(|e| e.to_string())?;
    let key = if api_key.trim().is_empty() { existing.api_key } else { api_key };
    let cfg = AiConfig { enabled, base_url, api_key: key, model };
    store.set_ai_config(&cfg).map_err(|e| e.to_string())
}

/// 测试 AI 连接（用当前已存配置；若前端传了新 key 则用新 key 测）。
#[tauri::command]
pub async fn test_ai_connection(
    state: State<'_, AppState>,
    base_url: String,
    api_key: String,
    model: String,
) -> Result<(), String> {
    // 取配置：先锁读出已存 key（前端未重填时用），随即释放锁再 await。
    let cfg = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let existing = store.ai_config().map_err(|e| e.to_string())?;
        let key = if api_key.trim().is_empty() { existing.api_key } else { api_key };
        AiConfig { enabled: true, base_url, api_key: key, model }
    };
    ai::test_connection(&cfg).await
}

/// 按需生成 AI 标题。
/// - 未启用 / 无 key → Ok(None)；
/// - 已有缓存且 !force → 返回缓存；
/// - 否则解析会话内容 → 调 AI → 缓存 → 返回新标题。
/// 任何网络/解析失败 → Err（前端静默降级，不更新标题）。
#[tauri::command]
pub async fn generate_ai_title(
    state: State<'_, AppState>,
    file_path: String,
    tool: String,
    force: bool,
) -> Result<Option<String>, String> {
    // 阶段 1：锁库读配置 + 缓存（不跨 await 持锁）。
    let cfg = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        if !force {
            if let Some(cached) = store.ai_title(&file_path).map_err(|e| e.to_string())? {
                return Ok(Some(cached));
            }
        }
        store.ai_config().map_err(|e| e.to_string())?
    };
    if !cfg.is_usable() {
        return Ok(None); // 未启用/无 key：静默不生成。
    }

    // 阶段 2：解析会话内容（纯文件读，无锁）。
    let (_, messages) = parse_transcript(&file_path, &tool)?;
    let excerpt = ai::excerpt_for_title(&messages, 3000);
    if excerpt.trim().is_empty() {
        return Ok(None);
    }

    // 阶段 3：await HTTP（无锁）。
    let title = ai::request_title(&cfg, &excerpt).await?;

    // 阶段 4：回锁写缓存。
    {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        store.set_ai_title(&file_path, &title).map_err(|e| e.to_string())?;
    }
    Ok(Some(title))
}

/// 读取会话的 AI 摘要缓存（只读，不触网）。无缓存返回 Ok(None)。
/// 选中会话时回显已有摘要用。
#[tauri::command]
pub fn get_ai_summary(
    state: State<'_, AppState>,
    file_path: String,
) -> Result<Option<ai::AiSummary>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.ai_summary(&file_path).map_err(|e| e.to_string())
}

/// 按需生成 AI 摘要（结构化三段式）。
/// - 未启用 / 无 key → Ok(None)；
/// - 已有缓存且 !force → 返回缓存；
/// - 否则解析会话内容 → 调 AI → 缓存 → 返回新摘要。
/// 任何网络/解析失败 → Err（前端静默降级）。与 generate_ai_title 同构（四阶段锁）。
#[tauri::command]
pub async fn generate_ai_summary(
    state: State<'_, AppState>,
    file_path: String,
    tool: String,
    force: bool,
) -> Result<Option<ai::AiSummary>, String> {
    // 阶段 1：锁库读配置 + 缓存（不跨 await 持锁）。
    let cfg = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        if !force {
            if let Some(cached) = store.ai_summary(&file_path).map_err(|e| e.to_string())? {
                return Ok(Some(cached));
            }
        }
        store.ai_config().map_err(|e| e.to_string())?
    };
    if !cfg.is_usable() {
        return Ok(None); // 未启用/无 key：静默不生成。
    }

    // 阶段 2：解析会话内容（纯文件读，无锁）。
    let (_, messages) = parse_transcript(&file_path, &tool)?;
    let excerpt = ai::excerpt_for_summary(&messages, ai::SUMMARY_EXCERPT_MAX);
    if excerpt.trim().is_empty() {
        return Ok(None);
    }

    // 阶段 3：await HTTP（无锁）。
    let summary = ai::request_summary(&cfg, &excerpt).await?;

    // 阶段 4：回锁写缓存（带模型名与生成时间）。
    let created_at = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
    {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        store
            .set_ai_summary(&file_path, &summary, &cfg.model, &created_at)
            .map_err(|e| e.to_string())?;
    }
    Ok(Some(summary))
}
