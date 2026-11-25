use std::sync::Arc;

use crate::{
    config::{AppState, StartupStatusCache},
    imap::ImapCommand,
    workers::SharedFetchManager,
};
use arc_swap::ArcSwap;
use tauri::Manager as _;
use tokio::{fs, sync::mpsc::UnboundedSender};

#[tracing::instrument(name = "command.logout.menu", skip(app))]
pub async fn logout_with_state(app: tauri::AppHandle) {
    // A later visit to the login page must not reuse the prior signed-in result.
    let startup_status_cache = app.state::<StartupStatusCache>();
    *startup_status_cache.status.lock().await = None;
    *startup_status_cache.warning.lock().await = None;

    // Invalidate local state before asking the worker to release remote resources.
    // The worker command is intentionally fire-and-forget: network shutdown must
    // never delay the user-visible logout transition.
    let app_state = app.state::<ArcSwap<AppState>>();
    app_state.store(Arc::new(AppState::Fresh));

    let fetch_manager = app.state::<SharedFetchManager>();
    fetch_manager.logout_token.lock().await.cancel();
    fetch_manager.refresh_token.lock().await.cancel();

    let imap_cmd_channel_tx = app.state::<UnboundedSender<ImapCommand>>();
    if let Err(err) = imap_cmd_channel_tx.send(ImapCommand::Logout) {
        tracing::warn!(error = %err, "Failed to queue IMAP resource cleanup during logout");
    }

    // Removing the config is the only durable operation that remains on the
    // foreground path; otherwise the account could be restored on next launch.
    match app.path().app_config_dir() {
        Ok(config_dir) => {
            let config_path = config_dir.join("config.json");
            match fs::remove_file(config_path).await {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    tracing::debug!("Config file was already removed during logout");
                }
                Err(err) => {
                    tracing::error!("Failed to remove config file during logout: {}", err);
                }
            }
        }
        Err(err) => tracing::error!("Failed to get config directory path during logout: {}", err),
    }
}
