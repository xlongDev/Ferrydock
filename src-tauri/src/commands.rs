use crate::state::AppState;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

/// Absolute path to the app's data directory (config.json + api-key live here).
#[tauri::command]
pub fn get_data_dir(state: State<'_, AppState>) -> String {
    state
        .engine
        .data_dir()
        .to_string_lossy()
        .to_string()
}

/// The gateway API key the desktop UI needs to call authenticated endpoints.
#[tauri::command]
pub fn get_api_key(state: State<'_, AppState>) -> String {
    state.engine.api_key()
}

/// Open a file or folder with the OS default handler (Finder / editor).
#[tauri::command]
pub fn open_path(app: AppHandle, path: String) -> Result<(), String> {
    app.opener()
        .open_path(path, None::<String>)
        .map_err(|e| format!("failed to open path: {e}"))
}

/// Quit the application.
#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}
