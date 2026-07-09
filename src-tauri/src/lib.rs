mod agent;
mod commands;
mod db;
mod hn;
mod scheduler;
mod tick;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // app.handle() already returns &AppHandle — no extra & (init_state takes &AppHandle).
            let state = commands::init_state(app.handle());
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::create_monitor,
            commands::list_monitors,
            commands::delete_monitor,
            commands::list_feed,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
