use tauri::{Manager as _, WebviewUrl, WebviewWindowBuilder};
use tokio::fs::{self, create_dir_all};

use crate::{
    config::{AppState, Config, InitStatus},
    error::Error,
    initialize_services,
};

#[tauri::command]
#[tracing::instrument(name = "command.config.setup", skip(app, imap_server, imap_port,))]
pub async fn config_setup(
    app: tauri::AppHandle,
    imap_server: String,
    imap_port: u16,
) -> Result<(), Error> {
    let rpc_server = std::env::var("RPC_SERVER").unwrap_or("0.0.0.0:5500".to_string());
    let rust_log = std::env::var("RUST_LOG").unwrap_or("info".to_string());
    let sqlite_db = std::env::var("SQLITE_DB").unwrap_or("emails.db".to_string());
    let otlp_collector_endpoint = std::env::var("OTLP_COLLECTOR_ENDPOINT")
        .unwrap_or("http://otel-collector:4317".to_string());

    let config = Config {
        rpc_server,
        imap_server,
        imap_port,
        sqlite_db,
        rust_log: Some(rust_log),
        otlp_collector_endpoint: Some(otlp_collector_endpoint),
    };

    let config_dir = app.path().app_config_dir()?;

    let config_json = serde_json::to_string_pretty(&config)?;

    // Create config directory if it doesn't exist
    create_dir_all(&config_dir).await?;

    let path = config_dir.join("config.json");
    fs::write(path, config_json).await?;

    Ok(())
}

/// Check the initialization status of the app on startup
/// Returns:
/// - Setup: if config file doesn't exist (show setup page with IMAP config)
/// - Login: if config exists but user not authenticated (show login page)
/// - SignedIn: if user is already authenticated (show main app)
#[tauri::command]
#[tracing::instrument(name = "command.config.check_status", skip(app))]
pub async fn check_init_status(app: tauri::AppHandle) -> Result<InitStatus, Error> {
    let config_dir = app.path().app_config_dir()?;
    let config_path = config_dir.join("config.json");

    if Config::init(&config_path).await.is_err() {
        return Ok(InitStatus::Setup);
    }

    // Config exists, check if user is already signed in
    let app_state = app.state::<std::sync::Arc<tokio::sync::RwLock<AppState>>>();
    let state = app_state.read().await;

    match &*state {
        AppState::Initialized(_) => Ok(InitStatus::SignedIn),
        AppState::Fresh => Ok(InitStatus::Login),
    }
}

/// Login command - used when config exists but user needs to authenticate
/// Only requires username and password
#[tauri::command]
#[tracing::instrument(name = "command.user.login", skip(app, password))]
pub async fn login(app: tauri::AppHandle, username: String, password: String) -> Result<(), Error> {
    let config_dir = app.path().app_config_dir()?;
    let config_path = config_dir.join("config.json");

    // Should never fail since we check for config existence before showing login page, but handle just in case
    let config = Config::init(&config_path).await.map_err(|_| {
        Error::Other("Configuration not found. Please run setup first.".to_string())
    })?;

    initialize_services(app.clone(), config, &username, &password).await?;

    // Fetch emails after initialization
    let app_state = app.state::<std::sync::Arc<tokio::sync::RwLock<AppState>>>();
    let mut state_guard = app_state.write().await;

    let initialized = state_guard.state_mut();

    crate::email_cache::fetch_emails(
        &mut initialized.imap_session,
        10,
        initialized.sqlite_pool.clone(),
    )
    .await?;

    Ok(())
}

/// Open a dedicated, larger main app window after successful login
/// and close the compact auth window.
#[tauri::command]
#[tracing::instrument(name = "command.window.open_main", skip(app, current_window))]
pub async fn open_main_window(
    app: tauri::AppHandle,
    current_window: tauri::WebviewWindow,
) -> Result<(), Error> {
    if let Some(main_window) = app.get_webview_window("main-app") {
        main_window.show()?;
        main_window.set_focus()?;
    } else {
        WebviewWindowBuilder::new(&app, "main-app", WebviewUrl::App("index.html".into()))
            .title("PhisherMan")
            .inner_size(1100.0, 760.0)
            .min_inner_size(900.0, 620.0)
            .resizable(true)
            .build()?;
    }

    current_window.close()?;

    Ok(())
}
