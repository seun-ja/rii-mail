use std::sync::Arc;

use arc_swap::ArcSwap;
use tauri::{LogicalSize, Manager as _, Size};
use tokio::{
    fs::{self, create_dir_all},
    sync::oneshot,
};

use crate::{
    auth::AppleKeychainManager,
    config::{self, AppState, Config, ImapClientConfig, InitStatus},
    db,
    error::Error,
    handlers::provider_from_imap_server,
    ReturningUserImapClientChannelTx,
};

#[tauri::command]
#[tracing::instrument(name = "command.config.setup", skip(app, imap_server, imap_port,))]
pub async fn config_setup(
    app: tauri::AppHandle,
    imap_server: String,
    imap_port: u16,
) -> Result<(), Error> {
    let rpc_server = std::env::var("RPC_SERVER").unwrap_or("0.0.0.0:5500".to_string());
    let sqlite_db = std::env::var("SQLITE_DB").unwrap_or("emails.db".to_string());

    let config = Config {
        rpc_server,
        imap_server,
        imap_port,
        sqlite_db,
        accounts: vec![],
    };

    let config_dir = app.path().app_config_dir()?;

    let config_json = serde_json::to_string_pretty(&config)?;

    let provider = provider_from_imap_server(&config.imap_server);

    db::init_db(&config_dir, &config.sqlite_db, &provider).await?;
    tracing::info!(
        "Database initialized successfully for provider: {}",
        provider.as_ref()
    );

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
pub async fn check_app_status(app: tauri::AppHandle) -> Result<InitStatus, Error> {
    let config_dir = app.path().app_config_dir()?;
    let config_path = config_dir.join("config.json");

    let Ok(config) = Config::init(&config_path).await else {
        ensure_compact_startup_window(&app)?;
        return Ok(InitStatus::Setup);
    };

    let app_service_name = app.config().identifier.clone();
    let apple_keychain_manager = AppleKeychainManager::new(&app_service_name);

    let (login_result_tx, login_result_rx) = oneshot::channel::<Result<(), String>>();

    if let Ok(password) = apple_keychain_manager
        .retrieve_password(&config.accounts.first().cloned().unwrap_or_default())
    {
        let imap_client_channel_tx = app.state::<ReturningUserImapClientChannelTx>();

        let sqlite_pool = db::db_pool(&config_dir, &config.sqlite_db).await?;

        if config.accounts.is_empty() {
            ::tracing::warn!("Config accounts field is empty, cannot attempt returning user login");
            ensure_compact_startup_window(&app)?;
            return Ok(InitStatus::Login);
        }

        let imap_client_config = ImapClientConfig {
            imap_server: config.imap_server.clone(),
            imap_port: config.imap_port,
            username: config.accounts.first().cloned().unwrap_or_default(),
            password,
            sqlite_pool: sqlite_pool.clone(),
            login_result_tx: Some(login_result_tx),
        };

        imap_client_channel_tx
            .0
            .send(imap_client_config)
            .map_err(|e| Error::ReturningUserImapConfigChannelSend(e.to_string()))?;
        ensure_expanded_startup_window(&app)?;

        match login_result_rx.await.map_err(|_| {
            Error::Other("Login worker failed to send authentication result".to_string())
        })? {
            Ok(()) => {
                ::tracing::info!("Returning user IMAP Login successful");
            }
            Err(message) => {
                ::tracing::error!(error = ?message, "Returning user login failed: {message}");
                ensure_compact_startup_window(&app)?;
                return Ok(InitStatus::Login);
            }
        }

        let initialized_state = config::InitializedState {
            rpc_llm_client: config::init_rpc(&config.rpc_server).await?,
            sqlite_pool,
            apple_keychain_manager,
        };

        let app_state = app.state::<ArcSwap<AppState>>();
        app_state.store(Arc::new(AppState::Initialized(Arc::new(initialized_state))));

        // Update app state to reflect already authenticated user
        Ok(InitStatus::SignedIn)
    } else {
        ::tracing::info!("No stored credentials found, user needs to login");
        ensure_compact_startup_window(&app)?;
        Ok(InitStatus::Login)
    }
}

fn ensure_expanded_startup_window(app: &tauri::AppHandle) -> Result<(), Error> {
    if let Some(window) = app.get_webview_window("main") {
        let expanded_size = Size::Logical(LogicalSize::new(1100.0, 760.0));
        let expanded_min_size = Size::Logical(LogicalSize::new(900.0, 620.0));

        window.set_resizable(true)?;
        window.set_max_size::<Size>(None)?;
        window.set_min_size(Some(expanded_min_size))?;
        window.set_size(expanded_size)?;
        window.center()?;
    }

    Ok(())
}

fn ensure_compact_startup_window(app: &tauri::AppHandle) -> Result<(), Error> {
    if let Some(window) = app.get_webview_window("main") {
        let compact_size = Size::Logical(LogicalSize::new(500.0, 700.0));

        window.set_resizable(false)?;
        window.set_max_size::<Size>(Some(compact_size))?;
        window.set_min_size(Some(compact_size))?;
        window.set_size(compact_size)?;
        window.center()?;
    }

    Ok(())
}

/// Open a dedicated, larger main app window after successful login
/// and close the compact auth window.
#[tauri::command]
#[tracing::instrument(name = "command.window.open_main", skip_all)]
pub async fn open_main_window(current_window: tauri::WebviewWindow) -> Result<(), Error> {
    let expanded_size = Size::Logical(LogicalSize::new(1100.0, 760.0));
    let expanded_min_size = Size::Logical(LogicalSize::new(900.0, 620.0));

    // Reuse the current auth window for post-login to avoid close/create race conditions.
    current_window.set_resizable(true)?;
    current_window.set_max_size::<Size>(None)?;
    current_window.set_min_size(Some(expanded_min_size))?;
    current_window.set_size(expanded_size)?;
    current_window.center()?;
    current_window.eval("window.location.replace('/')")?;
    current_window.show()?;
    current_window.set_focus()?;

    Ok(())
}
