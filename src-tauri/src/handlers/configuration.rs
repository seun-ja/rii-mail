use std::sync::Arc;

use arc_swap::ArcSwap;
use tauri::{Manager as _, WebviewUrl, WebviewWindowBuilder};
use tokio::{
    fs::{self, create_dir_all},
    sync::mpsc::UnboundedSender,
};

use crate::{
    config::{self, AppState, Config, ImapClientConfig, InitStatus},
    db,
    error::Error,
    imap::ImapCommand,
    // llm::{self, Providers},
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

    if Config::init(&config_path).await.is_err() {
        return Ok(InitStatus::Setup);
    }

    // Config exists, check if user is already signed in
    let app_state = app.state::<ArcSwap<AppState>>();
    let current_state = app_state.load();

    match current_state.as_ref() {
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

    let config = Config::init(&config_path).await.map_err(|_| {
        Error::Other("Configuration not found. Please run setup first.".to_string())
    })?;

    init_subscriber(
        &config.rust_log.unwrap_or_default(),
        &config.otlp_collector_endpoint.unwrap_or_default(),
    )
    .expect("Failed to initialize subscriber");

    let sqlite_pool = db::init_db(config_dir, &config.sqlite_db).await?;

    let imap_client_channel_tx = app.state::<UnboundedSender<ImapClientConfig>>();
    let imap_cmd_channel_tx = app.state::<UnboundedSender<ImapCommand>>();

    let imap_client_config = ImapClientConfig {
        username,
        password,
        imap_server: config.imap_server.clone(),
        imap_port: config.imap_port,
        sqlite_pool: sqlite_pool.clone(),
    };
    imap_client_channel_tx.send(imap_client_config)?;
    imap_cmd_channel_tx.send(ImapCommand::FetchEmails(config.email_cache_size))?;

    // TODO: Make configurable
    // let provider = Providers::LocalInference;
    // let model = "";
    // let system_message = "";
    // let api_key = "";
    // let max_tokens = None;
    // let temperature = None;

    // let llm_client_builder = llm::ProviderBuilder::new(provider, system_message, model);

    // let llm_client = llm_client_builder
    //     .api_key(api_key)
    //     .temperature(temperature)
    //     .max_tokens(max_tokens)
    //     .function_handler("predict".to_string())
    //     .script_name("local_inference".to_string())
    //     .build()
    //     .await?;

    let rpc_llm_client = config::init_rpc(&config.rpc_server).await?;

    let initialized_state = config::InitializedState {
        // _llm_client: llm_client,
        rpc_llm_client,
        sqlite_pool,
    };

    let app_state = app.state::<ArcSwap<AppState>>();
    app_state.store(Arc::new(AppState::Initialized(Arc::new(initialized_state))));

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
