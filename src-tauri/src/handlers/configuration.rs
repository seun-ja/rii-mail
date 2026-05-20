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
    sqlite_db: String,
    username: String,
    password: String,
) -> Result<(), Error> {
    let config = Config {
        rpc_server,
        imap_server,
        imap_port,
        sqlite_db,
        rust_log,
        otlp_collector_endpoint,
    };

    let config_dir = app.path().app_config_dir()?;

    let config_json = serde_json::to_string_pretty(&config)?;

    // Create config directory if it doesn't exist
    create_dir_all(&config_dir).await?;

    let path = config_dir.join("config.json");
    fs::write(path, config_json).await?;

    setup(app.clone(), config, username, password).await?;

    // Fetch emails after initialization
    let app_state = app.state::<std::sync::Arc<tokio::sync::RwLock<AppState>>>();
    let mut state_guard = app_state.write().await;

    let initialized = state_guard.state_mut();

    crate::email_cache::fetch_emails(
        &mut initialized.imap_session,
        1000,
        initialized.sqlite_pool.clone(),
    )
    .await?;

    Ok(())
}

#[tauri::command]
#[tracing::instrument(name = "command.config.check", skip(state))]
pub async fn is_initialized(
    app: tauri::AppHandle,
    state: tauri::State<'_, std::sync::Arc<tokio::sync::RwLock<AppState>>>,
    username: String,
    password: String,
) -> Result<bool, Error> {
    let config_dir = app.path().app_config_dir()?;

    if let Ok(config) = Config::init(&config_dir).await {
        setup(app, config, username, password).await?;
        return Ok(true);
    }

    let app_state = state.read().await;
    Ok(app_state.initialized())
}

async fn setup(
    app: tauri::AppHandle,
    config: Config,
    username: String,
    password: String,
) -> Result<(), Error> {
    initialize_services(app.clone(), config, &username, &password).await?;

    Ok(())
}
