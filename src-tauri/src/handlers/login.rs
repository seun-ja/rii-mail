use std::{path::PathBuf, sync::Arc};

use arc_swap::ArcSwap;
use tauri::Manager as _;
use tokio::{
    fs,
    sync::{mpsc::UnboundedSender, oneshot},
};
use tokio_util::sync::CancellationToken;

use crate::{
    auth::AppleKeychainManager,
    config::{self, AppState, Config, ImapClientConfig, InitializedState},
    db::{self, MailBox, Providers},
    error::Error,
    handlers::provider_from_imap_server,
    imap::ImapCommand,
    workers::SharedFetchManager,
    ImapClientChannelTx, LOGGED_IN, LOGGING_IN,
};

/// Login command - used when config exists but user needs to authenticate
/// Only requires username and password
#[tauri::command]
#[tracing::instrument(name = "command.user.login", skip(app, password))]
pub async fn login(app: tauri::AppHandle, username: String, password: String) -> Result<(), Error> {
    *LOGGING_IN.lock().await = true;

    let app_service_name = app.config().identifier.clone();
    let config_dir = app.path().app_config_dir()?;
    let config_path = config_dir.join("config.json");

    let mut config = Config::init(&config_path).await.map_err(|_| {
        Error::Other("Configuration not found. Please run setup first.".to_string())
    })?;

    config.accounts.push(username.clone());

    fs::write(config_path, serde_json::to_string_pretty(&config)?).await?;
    tracing::info!("Configuration updated with username and saved to disk");

    let (initialized_state, provider) =
        handle_initialization(config.clone(), config_dir, &app_service_name).await?;

    let imap_client_channel_tx = app.state::<ImapClientChannelTx>();
    let imap_cmd_channel_tx = app.state::<UnboundedSender<ImapCommand>>();
    let (login_result_tx, login_result_rx) = oneshot::channel::<Result<(), String>>();

    let imap_client_config = ImapClientConfig {
        username: username.clone(),
        password: password.clone(),
        imap_server: config.imap_server.clone(),
        imap_port: config.imap_port,
        sqlite_pool: initialized_state.sqlite_pool.clone(),
        login_result_tx: Some(login_result_tx),
    };
    tracing::info!("Sending IMAP client config to worker for authentication");
    imap_client_channel_tx.0.send(imap_client_config)?;

    match login_result_rx.await? {
        Ok(()) => {
            tracing::info!("IMAP Login successful");
        }
        Err(message) => {
            tracing::error!(error = ?message, "Login failed: {message}");
            return Err(Error::Authentication(message));
        }
    }

    // TODO: Make it OS agnostic to store credentials securely - for now we only have Apple Keychain implemented, but we can add Windows Credential Manager and Linux Secret Service in the future.
    // Store credentials securely in Apple Keychain
    initialized_state
        .apple_keychain_manager
        .store_password(&username, &password)?;

    let fetch_manager = app.state::<SharedFetchManager>();

    let fetch_token = {
        let mut parent = fetch_manager.logout_token.lock().await;

        *parent = CancellationToken::new();

        parent.child_token()
    };

    imap_cmd_channel_tx.send(ImapCommand::FetchEmails(
        MailBox::Inbox,
        provider.clone(),
        fetch_token,
    ))?;

    let initialized_state = config::InitializedState {
        rpc_llm_client: initialized_state.rpc_llm_client,
        sqlite_pool: initialized_state.sqlite_pool,
        apple_keychain_manager: initialized_state.apple_keychain_manager,
    };

    let app_state = app.state::<ArcSwap<AppState>>();
    app_state.store(Arc::new(AppState::Initialized(Arc::new(initialized_state))));

    tracing::info!("User Logged in and emails fetching initiated");

    *LOGGING_IN.lock().await = false;
    *LOGGED_IN.lock().await = true;

    Ok(())
}

async fn handle_initialization(
    config: Config,
    config_dir: PathBuf,
    app_service_name: &str,
) -> Result<(InitializedState, Providers), Error> {
    let provider = provider_from_imap_server(&config.imap_server);

    let sqlite_pool = db::db_pool(&config_dir, &config.sqlite_db).await?;

    let rpc_llm_client = config::init_rpc(&config.rpc_server).await?;

    let apple_keychain_manager = AppleKeychainManager::new(app_service_name);

    let initialized_state = config::InitializedState {
        rpc_llm_client,
        sqlite_pool,
        apple_keychain_manager,
    };

    Ok((initialized_state, provider))
}
