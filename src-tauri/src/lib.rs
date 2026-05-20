use tauri::Manager as _;

use crate::{
    config::{init_rpc, AppState, Config},
    error::Error,
    handlers::{check_init_status, config_setup, login, open_main_window, rater},
};

pub mod auth;
mod config;
pub mod db;
pub mod email_cache;
mod error;
mod handlers;
mod rpc;
mod session;
mod tracing;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub async fn run() {
    tauri::Builder::default()
        .manage(std::sync::Arc::new(tokio::sync::RwLock::new(
            AppState::Fresh,
        )))
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            rater,
            config_setup,
            check_init_status,
            login,
            open_main_window
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[::tracing::instrument(name = "initialize", skip(app, config, password))]
async fn initialize_services(
    app: tauri::AppHandle,
    config: Config,
    username: &str,
    password: &str,
) -> Result<(), Error> {
    tracing::init_subscriber(
        &config.rust_log.unwrap_or_default(),
        &config.otlp_collector_endpoint.unwrap_or_default(),
    )
    .expect("Failed to initialize subscriber");

    let rpc_client = init_rpc(&config.rpc_server)
        .await
        .map_err(|e| Error::Other(format!("Failed to initialize RPC Service: {}", e)))?;

    let imap_client = session::init_imap_client(&config.imap_server, config.imap_port)
        .await
        .map_err(|e| Error::Other(format!("Failed to initialize IMAP Session: {}", e)))?;

    let config_dir = app.path().app_config_dir()?;

    let sqlite_pool = db::init_db(config_dir, &config.sqlite_db)
        .await
        .map_err(|e| Error::Other(format!("Failed to initialize Database: {}", e)))?;

    let imap_session = auth::login(username, password, imap_client).await?;

    let initialized_state = config::InitializedState {
        rpc_client,
        imap_session,
        sqlite_pool,
    };

    let app_state = app.state::<std::sync::Arc<tokio::sync::RwLock<AppState>>>();
    let mut state = app_state.write().await;
    *state = AppState::Initialized(initialized_state);

    Ok(())
}
