use std::sync::Arc;

use crate::{
    config::{AppState, StartupStatusCache},
    db,
    imap::ImapCommand,
    workers::SharedFetchManager,
};
use arc_swap::ArcSwap;
use tauri::Manager as _;
use tokio::{
    fs,
    sync::{mpsc::UnboundedSender, Mutex},
};

#[tracing::instrument(name = "command.logout.menu", skip(app))]
pub async fn logout_with_state(app: tauri::AppHandle) {
    // A later visit to the login page must not reuse the prior signed-in result.
    *app.state::<StartupStatusCache>().0.lock().await = None;

    let imap_cmd_channel_tx = app.state::<UnboundedSender<ImapCommand>>();
    let _ = imap_cmd_channel_tx
        .send(ImapCommand::Logout)
        .map_err(|err| {
            tracing::error!("Failed to send logout command to IMAP worker: {}", err);
        });

    let fetch_manager = app.state::<SharedFetchManager>();
    fetch_manager.logout_token.lock().await.cancel();
    fetch_manager.refresh_token.lock().await.cancel();

    let config_dir = app.path().app_config_dir().unwrap_or_else(|err| {
        tracing::error!("Failed to get config directory path during logout: {}", err);
        std::path::PathBuf::from(".")
    });
    let config_path = config_dir.join("config.json");

    if let Err(err) = fs::remove_file(config_path).await {
        tracing::error!("Failed to remove config file during logout: {}", err);
    }

    let app_state = app.state::<ArcSwap<AppState>>();
    let logout_state_rx = app.state::<Mutex<tokio::sync::mpsc::UnboundedReceiver<()>>>();

    let mut logout_state_rx = logout_state_rx.lock().await;
    let _ = logout_state_rx.recv().await;
    tracing::info!("Received logout confirmation from session thread");

    // Clean up the database after logout
    let db_path = config_dir.join("data");
    _ = db::cleanup(db_path).await;

    app_state.store(Arc::new(AppState::Fresh));
}
