mod ai;
mod commands;
mod export;
mod fork;
mod indexer;
mod models;
mod parsers;
mod scanner;
mod store;
mod terminal;
mod watcher;

use commands::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = AppState::new().expect("failed to init app state");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .setup(|app| {
            // 启动文件监听：~/.claude 与 ~/.codex 变更时自动增量索引并通知前端刷新。
            watcher::spawn_watcher(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::scan,
            commands::list_projects,
            commands::list_sessions,
            commands::search,
            commands::get_transcript,
            commands::export_session,
            commands::resume_in_terminal,
            commands::delete_project,
            commands::delete_sessions,
            commands::hide_project,
            commands::unhide_project,
            commands::list_hidden,
            commands::list_starred,
            commands::set_star,
            commands::set_starred_all,
            commands::rename_session,
            commands::reveal_in_finder,
            commands::fork_tree,
            commands::get_ai_config,
            commands::set_ai_config,
            commands::test_ai_connection,
            commands::generate_ai_title,
            commands::get_ai_summary,
            commands::generate_ai_summary,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
