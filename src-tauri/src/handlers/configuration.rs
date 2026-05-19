use tauri::Manager as _;
use tokio::fs::{self, create_dir_all};

use crate::{
    config::{AppState, Config},
    error::Error,
    initialize_services,
};

#[tauri::command]
#[tracing::instrument(
    name = "command.config.setup",
    skip(
        app,
        rpc_server,
        imap_server,
        imap_port,
        rust_log,
        otlp_collector_endpoint
    )
)]
pub async fn config_setup(
    app: tauri::AppHandle,
    rpc_server: String,
    imap_server: String,
    imap_port: u16,
    rust_log: Option<String>,
    otlp_collector_endpoint: Option<String>,
) -> Result<(), Error> {
    let config = Config {
        rpc_server,
        imap_server,
        imap_port,
        rust_log,
        otlp_collector_endpoint,
    };

    let config_json = serde_json::to_string_pretty(&config)?;

    let config_dir = app.path().app_config_dir()?;

    // Create config directory if it doesn't exist
    create_dir_all(&config_dir).await?;

    let path = config_dir.join("config.json");
    fs::write(path, config_json).await?;

    initialize_services(app, config).await?;

    Ok(())
}

#[tauri::command]
#[tracing::instrument(name = "command.config.check", skip(state))]
pub async fn is_initialized(state: tauri::State<'_, AppState>) -> Result<bool, Error> {
    Ok(state.inner.read().await.is_some())
}
