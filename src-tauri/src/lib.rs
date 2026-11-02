mod commands;
mod state;

use ferrydock_core::Engine;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let dir = handle
                .path()
                .app_data_dir()
                .map_err(|e| e.to_string())?;
            let engine = tauri::async_runtime::block_on(async {
                Engine::build(dir)
                    .await
                    .expect("failed to build Ferrydock engine")
            });
            app.manage(state::AppState {
                engine: engine.clone(),
            });
            // Start the local OpenAI-compatible gateway. WorkBuddy connects here.
            tauri::async_runtime::spawn(async move {
                if let Err(e) = ferrydock_core::server::serve(engine).await {
                    tracing::error!("Ferrydock gateway exited: {e}");
                }
            });
            if let Err(e) = setup_tray(app) {
                tracing::warn!("failed to setup tray: {e}");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_data_dir,
            commands::get_api_key,
            commands::open_path,
            commands::quit_app,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Ferrydock");
}

/// Build a tray icon with a show / hide / quit menu. The dashboard stays usable
/// as a normal window; the tray is a convenience entry point on macOS.
fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::TrayIconBuilder;

    let show = MenuItem::with_id(app, "show", "Show Ferrydock", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "hide", "Hide", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Ferrydock", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &hide, &quit])?;

    let icon = app
        .default_window_icon()
        .cloned()
        .expect("default window icon must be present");

    let _tray = TrayIconBuilder::new()
        .icon(icon)
        .tooltip("Ferrydock")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "hide" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}
