use ferrydock_core::Engine;
use std::sync::Arc;

/// Shared application state, managed by Tauri. The React UI reaches the engine
/// through the local HTTP gateway (127.0.0.1:<port>); the commands in
/// `commands.rs` cover OS-level actions only (open paths, quit).
pub struct AppState {
    pub engine: Arc<Engine>,
}
