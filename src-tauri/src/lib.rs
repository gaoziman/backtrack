mod commands;
mod indexer;
mod models;
mod parsers;
mod scanner;
mod store;
mod terminal;

use commands::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = AppState::new().expect("failed to init app state");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::scan,
            commands::list_projects,
            commands::list_sessions,
            commands::search,
            commands::get_transcript,
            commands::resume_in_terminal,
            commands::delete_project,
            commands::delete_sessions,
            commands::hide_project,
            commands::unhide_project,
            commands::list_hidden,
            commands::list_starred,
            commands::set_star,
            commands::set_starred_all,
            commands::reveal_in_finder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
