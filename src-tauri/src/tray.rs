use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

/// Restore the main window: show it, un-minimize, and focus. Best-effort —
/// every call is ignore-on-error so a missing/closing window never panics.
fn show_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

/// Build the menu-bar tray icon with a Show / Quit menu. Left-clicking the
/// tray icon opens this menu (Tauri's default when a menu is attached), so
/// "Show HN Watch" is always one click away — the guaranteed window-restore
/// path. Quit calls `app.exit(0)`, which bypasses the close-to-tray handler.
pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show HN Watch", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit HN Watch", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        });

    // Reuse the app's existing icon for the tray glyph.
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }

    builder.build(app)?;
    Ok(())
}
