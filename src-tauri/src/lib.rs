mod agent;
mod commands;
mod db;
mod hn;
mod scheduler;
mod tick;
mod tray;

use tauri::{Manager, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            // app.handle() already returns &AppHandle — no extra & (init_state takes &AppHandle).
            let state = commands::init_state(app.handle());
            app.manage(state);

            // Menu-bar tray icon (Show / Quit) — keeps the app alive with the window closed.
            tray::build(app.handle())?;

            // Ask for notification permission up front (macOS shows the OS prompt once).
            {
                use tauri_plugin_notification::{NotificationExt, PermissionState};
                let n = app.notification();
                if !matches!(n.permission_state(), Ok(PermissionState::Granted)) {
                    let _ = n.request_permission();
                }
            }

            // Close-to-tray: the red button / Cmd-W hides the window instead of
            // quitting, so monitor workers keep ticking. Quit lives in the tray menu.
            if let Some(win) = app.get_webview_window("main") {
                let win_for_events = win.clone();
                win.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = win_for_events.hide();
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::create_monitor,
            commands::list_monitors,
            commands::delete_monitor,
            commands::list_feed,
            commands::claude_health,
            commands::recheck_claude,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // macOS fires Reopen when the Dock icon is clicked while the app is
            // running. Since close-to-tray only hides the window, re-show it here
            // — otherwise the Dock icon looks dead and only the tray can restore.
            if let tauri::RunEvent::Reopen { .. } = event {
                tray::show_main_window(app_handle);
            }
        });
}
