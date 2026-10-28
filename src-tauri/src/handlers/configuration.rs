use std::{sync::Arc, time::Duration};

use arc_swap::ArcSwap;
use futures::future::join_all;
use lettre::transport::smtp::authentication::Credentials;
use llm::tools::{SearchEmailsTool, ToolWrapper};
use tauri::{window::Color, LogicalSize, Manager as _, Size};
use tokio::{
    fs::{create_dir_all, try_exists},
    net::TcpStream,
    sync::oneshot,
    time::timeout,
};

use crate::{
    auth::AppleKeychainManager,
    config::{
        self, smtp_transport_client, Account, AppState, ChatGptConfig, Config, ImapClientConfig,
        InitStatus, StartupStatusCache,
    },
    db,
    error::Error,
    handlers::{provider_from_imap_server, refresh_chatgpt_token_if_needed},
    llm::{init_llm_agent, initialize_local_inference},
    workers::SharedFetchManager,
    ReturningUserImapClientChannelTx,
};

const SMTP_RELAY_PORT: u16 = 465;
const SMTP_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

pub(crate) fn smtp_relay_candidates(imap_server_url: &str) -> Vec<String> {
    let imap_server = imap_server_url
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let mut candidates = Vec::new();

    let mut add_candidate = |candidate: String| {
        if !candidate.is_empty() && !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    };

    match imap_server.as_str() {
        "imap.mail.yahoo.com" => add_candidate("smtp.mail.yahoo.com".to_string()),
        "imap.gmail.com" => add_candidate("smtp.gmail.com".to_string()),
        "outlook.office365.com" => add_candidate("smtp.office365.com".to_string()),
        "imap-mail.outlook.com" => add_candidate("smtp-mail.outlook.com".to_string()),
        _ => {}
    }

    if let Some(domain) = imap_server.strip_prefix("imap.") {
        add_candidate(format!("smtp.{domain}"));
        add_candidate(format!("mail.{domain}"));
    } else if let Some(domain) = imap_server.strip_prefix("imap-") {
        add_candidate(format!("smtp-{domain}"));
    } else if let Some(domain) = imap_server.strip_prefix("mail.") {
        // Many self-hosted mail systems expose IMAP and SMTP on one host.
        add_candidate(imap_server.clone());
        add_candidate(format!("smtp.{domain}"));
    } else if !imap_server.is_empty() && imap_server.split('.').count() == 2 {
        add_candidate(format!("smtp.{imap_server}"));
        add_candidate(format!("mail.{imap_server}"));
    }

    candidates
}

async fn smtp_relay_is_reachable(smtp_relay_url: &str) -> bool {
    matches!(
        timeout(
            SMTP_PROBE_TIMEOUT,
            TcpStream::connect((smtp_relay_url, SMTP_RELAY_PORT)),
        )
        .await,
        Ok(Ok(_))
    )
}

pub(crate) async fn resolve_smtp_relay_url(
    imap_server_url: &str,
    configured_smtp_relay_url: Option<&str>,
) -> Result<String, Error> {
    if let Some(configured) = configured_smtp_relay_url {
        let configured = configured.trim();
        if configured.is_empty() {
            return Err(Error::Other(
                "SMTP relay server is empty; provide a valid SMTP hostname".to_string(),
            ));
        }

        tracing::info!(
            smtp.host = configured,
            source = "configured",
            "Selected SMTP relay"
        );
        return Ok(configured.to_string());
    }

    let candidates = smtp_relay_candidates(imap_server_url);
    let probe_results = join_all(candidates.iter().map(|candidate| async move {
        let reachable = smtp_relay_is_reachable(candidate).await;
        tracing::debug!(
            smtp.host = candidate,
            smtp.port = SMTP_RELAY_PORT,
            reachable,
            "Probed derived SMTP relay candidate"
        );
        reachable
    }))
    .await;

    let selected_relay = candidates
        .into_iter()
        .zip(probe_results)
        .find_map(|(candidate, reachable)| reachable.then_some(candidate))
        .ok_or_else(|| {
            Error::SmtpDiscovery(format!(
                "No reachable SMTP server could be discovered from '{imap_server_url}'. Enter the SMTP server provided by your email administrator."
            ))
        })?;

    tracing::info!(
        smtp.host = %selected_relay,
        smtp.port = SMTP_RELAY_PORT,
        source = "discovered",
        "Selected reachable SMTP relay"
    );

    Ok(selected_relay)
}

#[tauri::command]
#[tracing::instrument(
    name = "command.config.setup",
    skip(app, imap_server_url, imap_port, smtp_relay_url),
    err
)]
pub async fn config_setup(
    app: tauri::AppHandle,
    imap_server_url: String,
    imap_port: u16,
    smtp_relay_url: Option<String>,
) -> Result<(), Error> {
    let sqlite_db = std::env::var("SQLITE_DB").unwrap_or("emails.db".to_string());
    let environment_smtp_relay_url = std::env::var("SMTP_RELAY_URL").ok();
    let configured_smtp_relay_url = smtp_relay_url
        .as_deref()
        .or(environment_smtp_relay_url.as_deref());
    let smtp_relay_url =
        resolve_smtp_relay_url(&imap_server_url, configured_smtp_relay_url).await?;
    let default_model = std::env::var("MODEL").unwrap_or("gemma4:e4b".to_string());
    let api_key = std::env::var("LLM_API_KEY").unwrap_or("ollama".to_string());
    let llm_provider = std::env::var("LLM_PROVIDER").ok();

    let config = Config {
        imap_server_url,
        smtp_relay_url,
        imap_port,
        sqlite_db,
        accounts: vec![],
        llm_provider,
        default_model,
        api_key,
        access_token: None,
        refresh_token: None,
        chatgpt: ChatGptConfig::default(),
    };

    let config_dir = app.path().app_config_dir()?;
    let data_dir = app.path().app_data_dir()?;

    let provider = provider_from_imap_server(&config.imap_server_url);

    db::init_db(&data_dir, &config.sqlite_db, &provider).await?;
    tracing::info!(
        "Database initialized successfully for provider: {}",
        provider.as_ref()
    );

    // Create config directory if it doesn't exist
    create_dir_all(&config_dir).await?;

    let path = config_dir.join("config.json");
    config
        .save(&path)
        .await
        .map_err(|error| Error::Other(format!("Could not save configuration: {error}")))?;
    tracing::info!(
        smtp.host = %config.smtp_relay_url,
        config.path = %path.display(),
        "Registered SMTP relay in app configuration"
    );

    // The startup check may have cached `Setup` while the configuration file
    // did not exist. Force the next status request to read the new config.
    *app.state::<StartupStatusCache>().0.lock().await = None;

    Ok(())
}

/// Check the initialization status of the app on startup
/// Returns:
/// - Setup: if config file doesn't exist (show setup page with IMAP config)
/// - Login: if config exists but user not authenticated (show login page)
/// - SignedIn: if user is already authenticated (show main app)
#[tauri::command]
#[tracing::instrument(name = "command.config.check_status", skip(app), err)]
pub async fn check_app_status(app: tauri::AppHandle) -> Result<InitStatus, Error> {
    // Multiple webview lifecycle events can invoke this command during startup.
    // Keep the guard for the full initialization so only the first request reads
    // the Keychain and the others reuse its successful result.
    let startup_status_cache = app.state::<StartupStatusCache>();
    let mut startup_status = startup_status_cache.0.lock().await;
    if let Some(status) = startup_status.as_ref() {
        tracing::debug!("Returning cached startup status without reading Apple Keychain");
        return Ok(status.clone());
    }

    tracing::debug!("Checking startup status and stored Apple Keychain credentials");

    let config_dir = app.path().app_config_dir()?;
    let data_dir = app.path().app_data_dir()?;
    let config_path = config_dir.join("config.json");

    if !try_exists(&config_path).await? {
        tracing::warn!(path = %config_path.display(), "Configuration not found; setup is required");
        let status = InitStatus::Setup;
        *startup_status = Some(status.clone());
        return Ok(status);
    }

    let Ok(mut config) = Config::init(&config_path).await else {
        return Ok(InitStatus::Setup);
    };
    refresh_chatgpt_token_if_needed(&mut config, &config_path).await?;

    let app_service_name = app.config().identifier.clone();
    let apple_keychain_manager = AppleKeychainManager::new(&app_service_name);

    if config.accounts.is_empty() {
        tracing::warn!("Config accounts field is empty, cannot attempt returning user login");
        return Ok(InitStatus::Login(None));
    }

    let (login_result_tx, login_result_rx) = oneshot::channel::<Result<(), String>>();
    let username = config.accounts.first().cloned().unwrap_or_default();

    if let Ok(password) = apple_keychain_manager.retrieve_password(&username) {
        let imap_client_channel_tx = app.state::<ReturningUserImapClientChannelTx>();

        let sqlite_pool = db::db_pool(&data_dir, &config.sqlite_db).await?;

        let credential = Credentials::new(username.clone(), password.clone());
        let smtp_transport_client = smtp_transport_client(&config.smtp_relay_url, credential)?;

        let imap_client_config = ImapClientConfig {
            imap_server_url: config.imap_server_url.clone(),
            imap_port: config.imap_port,
            username: username.clone(),
            password,
            sqlite_pool: sqlite_pool.clone(),
            login_result_tx: Some(login_result_tx),
        };

        imap_client_channel_tx
            .inner()
            .0
            .send(imap_client_config)
            .map_err(|e| Error::ReturningUserImapConfigChannelSend(e.to_string()))?;

        match login_result_rx.await.map_err(|_| {
            Error::Other("Login worker failed to send authentication result".to_string())
        })? {
            Ok(()) => {
                ::tracing::info!("Returning user IMAP Login successful");
            }
            Err(message) => {
                ::tracing::error!(error = ?message, "Returning user login failed: {message}");
                return Ok(InitStatus::Login(Some(message)));
            }
        }

        app.state::<SharedFetchManager>().reset_for_session().await;

        let resource_root = app.path().resource_dir()?;
        let initialized_state = config::InitializedState {
            local_inference: initialize_local_inference(&resource_root).await?,
            sqlite_pool,
            apple_keychain_manager,
            smtp_relay_url: config.smtp_relay_url.clone(),
            smtp_transport_client,
            accounts: vec![Account {
                name: None,
                email: username,
                provider: provider_from_imap_server(&config.imap_server_url),
            }],
            llm_agent: tokio::sync::RwLock::new(init_llm_agent(
                config.llm_provider.map(|provider| provider.as_str().into()),
                &config.default_model,
                config.access_token.as_deref().unwrap_or(&config.api_key),
                None,
                None,
                vec![ToolWrapper::new(SearchEmailsTool)],
            )?),
        };

        let app_state = app.state::<ArcSwap<AppState>>();
        app_state.store(Arc::new(AppState::Initialized(Arc::new(initialized_state))));

        // Update app state to reflect already authenticated user.
        let status = InitStatus::SignedIn;
        *startup_status = Some(status.clone());
        Ok(status)
    } else {
        ::tracing::info!("No stored credentials found, user needs to login");
        Ok(InitStatus::Login(None))
    }
}

#[tauri::command]
#[tracing::instrument(name = "command.window.finish_startup", skip_all, err)]
pub async fn finish_startup(
    app: tauri::AppHandle,
    startup_window: tauri::WebviewWindow,
) -> Result<(), Error> {
    let status = check_app_status(app.clone()).await?;
    let destination = match status {
        InitStatus::Setup => {
            ensure_compact_startup_window(&app)?;
            "/setup.html"
        }
        InitStatus::Login(_) => {
            ensure_compact_startup_window(&app)?;
            "/login.html"
        }
        InitStatus::SignedIn | InitStatus::ReturningSigned => {
            ensure_expanded_startup_window(&app)?;
            "/"
        }
    };

    let navigation_script = if destination == "/" {
        "window.location.replace('/');".to_string()
    } else {
        format!(
            "window.sessionStorage.setItem('riimail.startupStatusChecked', 'true'); \
             window.location.replace('{destination}');"
        )
    };

    startup_window.eval(&navigation_script)?;
    startup_window.show()?;
    startup_window.set_focus()?;

    Ok(())
}

fn ensure_expanded_startup_window(app: &tauri::AppHandle) -> Result<(), Error> {
    if let Some(window) = app.get_webview_window("main") {
        let expanded_size = Size::Logical(LogicalSize::new(1100.0, 760.0));
        let expanded_min_size = Size::Logical(LogicalSize::new(900.0, 620.0));

        window.set_min_size::<Size>(None)?;
        window.set_max_size::<Size>(None)?;
        window.set_background_color(Some(Color(244, 247, 251, 255)))?;
        window.set_decorations(true)?;
        window.set_shadow(true)?;
        window.set_minimizable(true)?;
        window.set_resizable(true)?;
        window.set_maximizable(true)?;
        window.set_size(expanded_size)?;
        window.set_min_size(Some(expanded_min_size))?;
        window.center()?;
    }

    Ok(())
}

fn ensure_compact_startup_window(app: &tauri::AppHandle) -> Result<(), Error> {
    if let Some(window) = app.get_webview_window("main") {
        let compact_size = Size::Logical(LogicalSize::new(600.0, 640.0));

        window.set_min_size::<Size>(None)?;
        window.set_max_size::<Size>(None)?;
        window.set_background_color(Some(Color(244, 247, 251, 255)))?;
        window.set_decorations(true)?;
        window.set_shadow(true)?;
        window.set_minimizable(true)?;
        window.set_resizable(false)?;
        window.set_maximizable(false)?;
        window.set_size(compact_size)?;
        window.set_min_size(Some(compact_size))?;
        window.set_max_size::<Size>(Some(compact_size))?;
        window.center()?;
    }

    Ok(())
}

#[tauri::command]
#[tracing::instrument(name = "command.window.enable_main_controls", skip_all, err)]
pub fn enable_main_window_controls(current_window: tauri::WebviewWindow) -> Result<(), Error> {
    // The splash and authentication pages intentionally lock the shared window.
    // Always clear those constraints when the mail UI is active, regardless of
    // which navigation path reached it.
    current_window.set_max_size::<Size>(None)?;
    current_window.set_resizable(true)?;
    current_window.set_maximizable(true)?;
    current_window.set_minimizable(true)?;
    current_window.set_decorations(true)?;
    current_window.set_shadow(true)?;

    Ok(())
}

/// Open a dedicated, larger main app window after successful login
/// and close the compact auth window.
#[tauri::command]
#[tracing::instrument(name = "command.window.open_main", skip_all, err)]
pub async fn open_main_window(current_window: tauri::WebviewWindow) -> Result<(), Error> {
    let expanded_size = Size::Logical(LogicalSize::new(1100.0, 760.0));
    let expanded_min_size = Size::Logical(LogicalSize::new(900.0, 620.0));

    // Reuse the current auth window for post-login to avoid close/create race conditions.
    current_window.set_background_color(Some(Color(244, 247, 251, 255)))?;
    current_window.set_resizable(true)?;
    current_window.set_maximizable(true)?;
    current_window.set_max_size::<Size>(None)?;
    current_window.set_min_size(Some(expanded_min_size))?;
    current_window.set_size(expanded_size)?;
    current_window.center()?;
    current_window.eval("window.location.replace('/')")?;
    current_window.show()?;
    current_window.set_focus()?;

    Ok(())
}
