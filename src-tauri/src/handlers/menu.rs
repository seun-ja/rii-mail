use std::sync::Arc;

use crate::{config::AppState, db, error::Error, imap::ImapCommand, workers::SharedFetchManager};
use arc_swap::ArcSwap;
use tauri::Manager as _;
use tokio::{
    fs,
    sync::{mpsc::UnboundedSender, Mutex},
};

#[tracing::instrument(name = "command.logout.menu", skip(app))]
pub async fn logout_with_state(app: tauri::AppHandle) -> Result<(), Error> {
    let imap_cmd_channel_tx = app.state::<UnboundedSender<ImapCommand>>();
    imap_cmd_channel_tx.send(ImapCommand::Logout)?;

    let fetch_manager = app.state::<SharedFetchManager>();
    fetch_manager.logout_token.lock().await.cancel();
    fetch_manager.refresh_token.lock().await.cancel();

    let config_dir = app.path().app_config_dir()?;
    let config_path = config_dir.join("config.json");

    if let Err(err) = fs::remove_file(config_path).await {
        if err.kind() != std::io::ErrorKind::NotFound {
            return Err(err.into());
        }
    }

    let app_state = app.state::<ArcSwap<AppState>>();
    let logout_state_rx = app.state::<Mutex<tokio::sync::mpsc::UnboundedReceiver<()>>>();

    let mut logout_state_rx = logout_state_rx.lock().await;
    let _ = logout_state_rx.recv().await;
    let db_path = config_dir.join("data");
    db::cleanup(db_path).await?;
    tracing::info!("Received logout confirmation from session thread");

    app_state.store(Arc::new(AppState::Fresh));

    Ok(())
}
