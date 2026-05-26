use std::{path::PathBuf, sync::Arc};

use arc_swap::ArcSwap;
use tauri::{LogicalSize, Manager as _, Size};
use tokio::{
    fs::{self, create_dir_all},
    sync::{mpsc::UnboundedSender, oneshot},
};

use crate::{
    auth::AppleKeychainManager,
    config::{
        self, AppState, Config, ImapClientConfig, InitStatus, InitializedState,
        ReturningUserImapClientConfig,
    },
    db::{self, MailBox, Providers},
    error::Error,
    imap::ImapCommand,
    tracing::init_subscriber,
};

#[tauri::command]
#[tracing::instrument(name = "command.config.setup", skip(app, imap_server, imap_port,))]
pub async fn config_setup(
    app: tauri::AppHandle,
    imap_server: String,
    imap_port: u16,
) -> Result<(), Error> {
    dotenv::dotenv().ok();

    let rpc_server = std::env::var("RPC_SERVER").unwrap_or("0.0.0.0:5500".to_string());
    let rust_log = std::env::var("RUST_LOG").unwrap_or("info".to_string());
    let sqlite_db = std::env::var("SQLITE_DB").unwrap_or("emails.db".to_string());
    let otlp_collector_endpoint =
        std::env::var("OTLP_COLLECTOR_ENDPOINT").unwrap_or("http://0.0.0.0:4317".to_string());
    let email_cache_size = std::env::var("EMAIL_CACHE_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);

    let config = Config {
        rpc_server,
        imap_server,
        imap_port,
        sqlite_db,
        accounts: vec![],
        rust_log: Some(rust_log),
        otlp_collector_endpoint: Some(otlp_collector_endpoint),
        email_cache_size,
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

    let config = Config::init(&config_path).await.ok();

    if config.is_none() {
        return Ok(InitStatus::Setup);
    }

    // If database exists, we can attempt to authenticate user with stored credentials and skip login page
    if config_dir.join("data").exists() {
        let config = config.unwrap();

        let imap_client_channel_tx = app.state::<UnboundedSender<ReturningUserImapClientConfig>>();
        let (login_result_tx, login_result_rx) = oneshot::channel::<Result<(), String>>();

        let app_service_name = app.config().identifier.clone();
        let apple_keychain_manager = AppleKeychainManager::new(&app_service_name);

        let provider = if config.imap_server.contains("yahoo") {
            db::Providers::Yahoo
        } else {
            db::Providers::Gmail
        };

        let sqlite_pool = db::init_db(config_dir, &config.sqlite_db, &provider).await?;

        if config.accounts.is_empty() {
            ::tracing::warn!("Config accounts field is empty, cannot attempt returning user login");
            return Ok(InitStatus::Login);
        }

        let imap_client_config = ReturningUserImapClientConfig {
            imap_server: config.imap_server.clone(),
            imap_port: config.imap_port,
            username: config.accounts.first().cloned().unwrap_or_default(),
            sqlite_pool: sqlite_pool.clone(),
            apple_keychain_manager: apple_keychain_manager.clone(),
            login_result_tx: Some(login_result_tx),
        };

        imap_client_channel_tx.send(imap_client_config)?;
        ensure_expanded_startup_window(&app)?;

        match login_result_rx.await.map_err(|_| {
            Error::Other("Login worker failed to send authentication result".to_string())
        })? {
            Ok(()) => {
                ::tracing::info!("Returning user IMAP Login successful");
            }
            Err(message) => {
                ::tracing::error!(error = ?message, "Returning user login failed: {message}");
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
        return Ok(InitStatus::SignedIn);
    }

    Ok(InitStatus::Login)
}

fn ensure_expanded_startup_window(app: &tauri::AppHandle) -> Result<(), Error> {
    if let Some(window) = app.get_webview_window("main") {
        let expanded_size = Size::Logical(LogicalSize::new(1100.0, 760.0));
        let expanded_min_size = Size::Logical(LogicalSize::new(900.0, 620.0));

        window.set_resizable(true)?;
        window.set_max_size::<Size>(None)?;
        window.set_min_size(Some(expanded_min_size))?;
        window.set_size(expanded_size)?;
    }

    Ok(())
}

/// Login command - used when config exists but user needs to authenticate
/// Only requires username and password
#[tauri::command]
#[tracing::instrument(name = "command.user.login", skip(app, password))]
pub async fn login(app: tauri::AppHandle, username: String, password: String) -> Result<(), Error> {
    let app_service_name = app.config().identifier.clone();
    let config_dir = app.path().app_config_dir()?;
    let config_path = config_dir.join("config.json");

    let mut config = Config::init(&config_path).await.map_err(|_| {
        Error::Other("Configuration not found. Please run setup first.".to_string())
    })?;

    config.accounts = vec![username.clone()];

    fs::write(config_path, serde_json::to_string_pretty(&config)?).await?;

    let (initialized_state, provider) =
        handle_initialization(config.clone(), config_dir, &app_service_name).await?;

    // TODO: Make it OS agnostic to store credentials securely - for now we only have Apple Keychain implemented, but we can add Windows Credential Manager and Linux Secret Service in the future.
    // Store credentials securely in Apple Keychain
    initialized_state
        .apple_keychain_manager
        .store_password(&username, &password)?;

    let imap_client_channel_tx = app.state::<UnboundedSender<ImapClientConfig>>();
    let imap_cmd_channel_tx = app.state::<UnboundedSender<ImapCommand>>();
    let (login_result_tx, login_result_rx) = oneshot::channel::<Result<(), String>>();

    let imap_client_config = ImapClientConfig {
        username,
        password,
        imap_server: config.imap_server.clone(),
        imap_port: config.imap_port,
        sqlite_pool: initialized_state.sqlite_pool.clone(),
        login_result_tx: Some(login_result_tx),
    };
    imap_client_channel_tx.send(imap_client_config)?;

    match login_result_rx.await.map_err(|_| {
        Error::Other("Login worker failed to send authentication result".to_string())
    })? {
        Ok(()) => {
            ::tracing::info!("IMAP Login successful");
        }
        Err(message) => {
            ::tracing::error!(error = ?message, "Login failed: {message}");
            return Err(Error::Authentication(message));
        }
    }

    imap_cmd_channel_tx.send(ImapCommand::FetchEmails(
        config.email_cache_size,
        MailBox::Inbox,
        provider.clone(),
    ))?;

    // move to another thread
    imap_cmd_channel_tx.send(ImapCommand::FetchEmails(
        config.email_cache_size,
        MailBox::Sent,
        provider,
    ))?;

    let initialized_state = config::InitializedState {
        rpc_llm_client: initialized_state.rpc_llm_client,
        sqlite_pool: initialized_state.sqlite_pool,
        apple_keychain_manager: initialized_state.apple_keychain_manager,
    };

    let app_state = app.state::<ArcSwap<AppState>>();
    app_state.store(Arc::new(AppState::Initialized(Arc::new(initialized_state))));

    ::tracing::info!("User Logged in and emails fetching initiated");

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
    current_window.eval("window.location.replace('/')")?;
    current_window.show()?;
    current_window.set_focus()?;

    Ok(())
}

async fn handle_initialization(
    config: Config,
    config_dir: PathBuf,
    app_service_name: &str,
) -> Result<(InitializedState, Providers), Error> {
    init_subscriber(
        &config.rust_log.clone().unwrap_or_default(),
        &config.otlp_collector_endpoint.clone().unwrap_or_default(),
    )
    .map_err(|_| Error::Other("Failed to initialize subscriber".to_string()))?;

    let provider = if config.imap_server.contains("yahoo") {
        db::Providers::Yahoo
    } else {
        db::Providers::Gmail
    };

    let sqlite_pool = db::init_db(config_dir, &config.sqlite_db, &provider).await?;

    let rpc_llm_client = config::init_rpc(&config.rpc_server).await?;

    let apple_keychain_manager = AppleKeychainManager::new(app_service_name);

    let initialized_state = config::InitializedState {
        rpc_llm_client,
        sqlite_pool,
        apple_keychain_manager,
    };

    Ok((initialized_state, provider))
}
