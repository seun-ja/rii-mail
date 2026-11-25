use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use async_native_tls::TlsStream;
use sqlx::SqlitePool;
use tauri::Manager as _;
use tokio::sync::oneshot::Sender;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::{self, MissedTickBehavior};
use tokio::{net::TcpStream, sync::mpsc::UnboundedReceiver};
use tracing::Instrument as _;

use crate::config::{AppState, ImapClientConfig};
use crate::db::{self, check_email_db_empty, set_email_read, MailBox, Provider};
use crate::email_cache::{
    fetch_attachment_on_server, fetch_emails, fetch_latest, set_email_read_on_server,
    SharedImapSession,
};
use crate::error::{friendly_login_error_message, Error};
use crate::imap::{init_imap_client, ImapCommand, RefreshSummary};
use crate::{auth, FETCH_MANAGER, LOGGED_IN, LOGGING_IN};

const RESOURCE_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

async fn logout_imap_session(session: Option<SharedImapSession>, session_name: &'static str) {
    let Some(session) = session else {
        return;
    };

    match time::timeout(RESOURCE_SHUTDOWN_TIMEOUT, async {
        let mut session = session.lock().await;
        session.logout().await
    })
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            tracing::warn!(
                ?error,
                session = session_name,
                "Failed to logout IMAP session"
            );
        }
        Err(_) => {
            tracing::warn!(
                session = session_name,
                timeout_seconds = RESOURCE_SHUTDOWN_TIMEOUT.as_secs(),
                "Timed out logging out IMAP session; dropping the connection"
            );
        }
    }
}

pub(crate) async fn cleanup_logout_resources(
    pool: Option<SqlitePool>,
    data_path: Option<PathBuf>,
    primary_session: Option<SharedImapSession>,
    background_session: Option<SharedImapSession>,
) {
    let cleanup_cache = async {
        if let Some(pool) = pool {
            if time::timeout(RESOURCE_SHUTDOWN_TIMEOUT, pool.close())
                .await
                .is_err()
            {
                tracing::warn!(
                    timeout_seconds = RESOURCE_SHUTDOWN_TIMEOUT.as_secs(),
                    "Timed out closing the email database pool"
                );
            }
            drop(pool);
        }

        if let Some(data_path) = data_path {
            if let Err(error) = db::cleanup(data_path.clone()).await {
                tracing::warn!(
                    ?error,
                    path = %data_path.display(),
                    "Failed to remove cached email data after logout"
                );
            }
        }
    };

    tokio::join!(
        cleanup_cache,
        logout_imap_session(primary_session, "primary"),
        logout_imap_session(background_session, "background"),
    );
}

async fn await_logout_cleanup(logout_cleanup: &mut Option<JoinHandle<()>>) {
    if let Some(task) = logout_cleanup.take() {
        if let Err(error) = task.await {
            tracing::warn!(?error, "Logout cleanup task failed");
        }
    }
}

fn refresh_ticker(interval_seconds: u8) -> time::Interval {
    let period = Duration::from_secs(u64::from(interval_seconds.max(1)));
    time::interval_at(time::Instant::now() + period, period)
}

pub(crate) fn is_imap_connection_error(error: &Error) -> bool {
    matches!(
        error,
        Error::Imap(async_imap::error::Error::Io(_) | async_imap::error::Error::ConnectionLost)
    )
}

async fn reconnect_imap_session(
    username: &str,
    keychain_manager: &auth::AppleKeychainManager,
    imap_endpoint: Option<&(String, u16)>,
) -> Result<SharedImapSession, Error> {
    let password = keychain_manager.retrieve_password(username)?;
    let (server, port) = imap_endpoint
        .ok_or_else(|| Error::Other("IMAP endpoint is not initialized".to_string()))?;
    let client = init_imap_client(server, *port).await?;
    let session = auth::login(username, &password, Some(client)).await?;

    Ok(Arc::new(Mutex::new(session)))
}

fn reconnect_account(
    app: &tauri::AppHandle,
) -> Result<(String, auth::AppleKeychainManager), Error> {
    let state = app.state::<ArcSwap<AppState>>();
    let current_state = state.load();
    let initialized = match current_state.as_ref() {
        AppState::Initialized(initialized) => initialized,
        AppState::Fresh => {
            return Err(Error::Other(
                "Application state is not initialized".to_string(),
            ));
        }
    };
    let username = initialized
        .accounts
        .first()
        .map(|account| account.email.clone())
        .ok_or_else(|| Error::Other("No email account is configured".to_string()))?;

    Ok((username, initialized.apple_keychain_manager.clone()))
}

async fn reconnect_periodic_session(
    app: &tauri::AppHandle,
    imap_endpoint: Option<&(String, u16)>,
) -> Result<SharedImapSession, Error> {
    let (username, keychain_manager) = reconnect_account(app)?;
    reconnect_imap_session(&username, &keychain_manager, imap_endpoint).await
}

async fn refresh_periodic_mailbox(
    app: &tauri::AppHandle,
    session: SharedImapSession,
    pool: &SqlitePool,
    mailbox: &MailBox,
    provider: &Provider,
    cancel_token: tokio_util::sync::CancellationToken,
    imap_endpoint: Option<&(String, u16)>,
) -> Result<Option<SharedImapSession>, Error> {
    let result = fetch_latest(
        app.clone(),
        session,
        pool,
        mailbox,
        provider,
        cancel_token.clone(),
    )
    .await;

    match result {
        Err(error) if is_imap_connection_error(&error) => {
            tracing::warn!(
                error = %error,
                mailbox = mailbox.as_ref(),
                "Periodic refresh lost its IMAP connection; reconnecting"
            );
            let new_session = reconnect_periodic_session(app, imap_endpoint).await?;
            fetch_latest(
                app.clone(),
                new_session.clone(),
                pool,
                mailbox,
                provider,
                cancel_token,
            )
            .await?;

            Ok(Some(new_session))
        }
        Ok(_) => Ok(None),
        Err(error) => Err(error),
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn worker(
    app: tauri::AppHandle,
    mut imap_client_channel_rx: UnboundedReceiver<ImapClientConfig>,
    mut imap_client_returning_user_channel_rx: UnboundedReceiver<ImapClientConfig>,
    mut imap_cmd_channel_rx: UnboundedReceiver<ImapCommand>,
    inbox_initial_email_populated_tx: Arc<Mutex<Option<Sender<()>>>>,
    sent_initial_email_populated_tx: Arc<Mutex<Option<Sender<()>>>>,
    spam_initial_email_populated_tx: Arc<Mutex<Option<Sender<()>>>>,
) {
    let worker_span = tracing::info_span!("worker.imap.run");

    tokio::spawn(
        async move {
        let mut initialized_session: Option<SharedImapSession> = None;
        let mut initialized_session_background: Option<SharedImapSession> = None;

        let mut pool: Option<SqlitePool> = None;
        let mut imap_client_channel_open = true;
        let mut imap_cmd_channel_open = true;
        let mut providers: HashSet<Provider> = HashSet::new();
        let mut initial_fetch_completed: HashSet<Provider> = HashSet::new();
        let mut database_initialized = false;

        let mut imap_endpoint: Option<(String, u16)> = None;

        let logged_in_finalised = {
            let logged_in = LOGGED_IN.lock().await;
            let logging_in = LOGGING_IN.lock().await;
            *logged_in && !*logging_in
        };

        let mut logging_out = false;
        let mut logout_cleanup: Option<JoinHandle<()>> = None;

        // The worker starts before AppState is initialized. Login configuration
        // replaces this placeholder ticker before periodic refresh is enabled.
        let mut ticker = refresh_ticker(1);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

        while imap_client_channel_open || imap_cmd_channel_open || logged_in_finalised {
            tokio::select! {
                maybe_config = imap_client_channel_rx.recv(), if imap_client_channel_open => {
                    let Some(mut config) = maybe_config else {
                        imap_client_channel_open = false;
                        tracing::warn!("IMAP config channel closed");
                        continue;
                    };
                    await_logout_cleanup(&mut logout_cleanup).await;
                    logging_out = false;
                    ticker = refresh_ticker(config.auto_refresh_interval);
                    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

                    let background_config = ImapClientConfig {
                        username: config.username.clone(),
                        password: config.password.clone(),
                        imap_server_url: config.imap_server_url.clone(),
                        imap_port: config.imap_port,
                        sqlite_pool: config.sqlite_pool.clone(),
                        auto_refresh_interval: config.auto_refresh_interval,
                        login_result_tx: None,
                    };
                    imap_endpoint = Some((config.imap_server_url.clone(), config.imap_port));

                    tracing::info!("Initiating IMAP session");
                    match init_imap_client(&config.imap_server_url, config.imap_port).await {
                        Ok(imap_client) => {
                            login(config, &mut initialized_session, imap_client, &mut pool).await;
                        }
                        Err(err) => {
                            tracing::error!(error = ?err, "Failed to initialize IMAP client");

                            if let Some(tx) = config.login_result_tx.take() {
                                let _ = tx.send(Err(friendly_login_error_message("init", &err.to_string())));
                            }
                            continue;
                        }
                    }

                    match init_imap_client(&background_config.imap_server_url, background_config.imap_port).await {
                        Ok(imap_client) => {
                            login(
                                background_config,
                                &mut initialized_session_background,
                                imap_client,
                                &mut pool,
                            )
                            .await;
                        }
                        Err(err) => {
                            tracing::error!(error = ?err, "Failed to initialize background IMAP client");
                        }
                    }
                }
                maybe_returning_user_config = imap_client_returning_user_channel_rx.recv() => {
                    tracing::info!("Received IMAP config for returning user");
                    let Some(mut config) = maybe_returning_user_config else {
                        tracing::warn!("Returning user IMAP config channel closed");
                        continue;
                    };
                    await_logout_cleanup(&mut logout_cleanup).await;
                    logging_out = false;
                    ticker = refresh_ticker(config.auto_refresh_interval);
                    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

                    let background_config = ImapClientConfig {
                        username: config.username.clone(),
                        password: config.password.clone(),
                        imap_server_url: config.imap_server_url.clone(),
                        imap_port: config.imap_port,
                        sqlite_pool: config.sqlite_pool.clone(),
                        auto_refresh_interval: config.auto_refresh_interval,
                        login_result_tx: None,
                    };
                    imap_endpoint = Some((config.imap_server_url.clone(), config.imap_port));

                    tracing::info!("Initiating IMAP session for returning user");
                    // TODO: A retry mechanism or timeout
                    match init_imap_client(&config.imap_server_url, config.imap_port).await {
                        Ok(imap_client) => {
                            login(config, &mut initialized_session, imap_client, &mut pool).await;
                        }
                        Err(err) => {
                            tracing::error!(error = ?err, "Failed to initialize IMAP client for returning user");

                            if let Some(tx) = config.login_result_tx.take() {
                                let _ = tx.send(Err(friendly_login_error_message("init", &err.to_string())));
                            }
                            continue;
                        }
                    }

                    match init_imap_client(&background_config.imap_server_url, background_config.imap_port).await {
                        Ok(imap_client) => {
                            login(
                                background_config,
                                &mut initialized_session_background,
                                imap_client,
                                &mut pool,
                            )
                            .await;

                            tracing::info!("IMAP sessions initialized for returning user");
                        }
                        Err(err) => {
                            tracing::error!(error = ?err, "Failed to initialize background IMAP client for returning user");
                        }
                    }
                }
                maybe_cmd = imap_cmd_channel_rx.recv(), if imap_cmd_channel_open => {
                    let Some(cmd) = maybe_cmd else {
                        imap_cmd_channel_open = false;
                        continue;
                    };

                    match cmd {
                        ImapCommand::FetchEmails(provider, cancel_token) => {
                            if initial_fetch_completed.contains(&provider) {
                                tracing::info!(
                                    provider = provider.as_ref(),
                                    "Skipping duplicate initial mailbox fetch command"
                                );
                                continue;
                            }

                            let Some(session) = initialized_session.as_ref() else {
                                tracing::warn!("FetchEmails ignored: IMAP session is not initialized");
                                continue;
                            };

                            let Some(pool_ref) = pool.as_ref() else {
                                tracing::warn!("FetchEmails ignored: sqlite pool is not initialized");
                                continue;
                            };

                            let inbox_key = format!(
                                "{}_{}",
                                provider.as_ref(),
                                MailBox::Inbox.as_ref()
                            );
                            let has_cached_inbox = match check_email_db_empty(
                                pool_ref,
                                &inbox_key,
                            )
                            .await
                            {
                                Ok(has_cached_inbox) => has_cached_inbox,
                                Err(error) => {
                                    tracing::warn!(
                                        ?error,
                                        provider = provider.as_ref(),
                                        "Could not inspect the cached Inbox; running a full mailbox sync"
                                    );
                                    false
                                }
                            };

                            if has_cached_inbox {
                                initial_fetch_completed.insert(provider.clone());
                                providers.insert(provider.clone());
                                database_initialized = true;

                                for sender in [
                                    &inbox_initial_email_populated_tx,
                                    &sent_initial_email_populated_tx,
                                    &spam_initial_email_populated_tx,
                                ] {
                                    if let Some(tx) = sender.lock().await.take() {
                                        let _ = tx.send(());
                                    }
                                }

                                tracing::info!(
                                    provider = provider.as_ref(),
                                    "Cached Inbox is ready; periodic incremental refresh enabled"
                                );
                                continue;
                            }

                            let Some(session_background) = initialized_session_background.as_ref() else {
                                tracing::warn!("FetchEmails ignored: background IMAP session is not initialized");
                                continue;
                            };

                            let inbox_tx = inbox_initial_email_populated_tx.clone();
                            let sent_tx = sent_initial_email_populated_tx.clone();
                            let spam_tx = spam_initial_email_populated_tx.clone();

                            match fetch_emails(
                                session.clone(),
                                Some(session_background.clone()),
                                pool_ref,
                                provider.clone(),
                                cancel_token,
                                inbox_tx,
                                sent_tx,
                                spam_tx,
                            )
                            .await
                            {
                                Ok(_) => {
                                    initial_fetch_completed.insert(provider.clone());
                                    providers.insert(provider.clone());
                                    database_initialized = true;
                                    tracing::info!(
                                        provider = provider.as_ref(),
                                        "Initial sync completed and periodic refresh enabled"
                                    );
                                }
                                Err(Error::ThreadCancel) => {
                                    tracing::info!("Initial sync cancelled");
                                }
                                Err(err) => {
                                    tracing::error!(error=%err, "Initial sync failed");
                                }
                            }
                        }
                        ImapCommand::Logout => {
                            let primary_session = initialized_session.take();
                            let background_session = initialized_session_background.take();
                            let cleanup_pool = pool.take();

                            imap_endpoint = None;
                            providers.clear();
                            initial_fetch_completed.clear();
                            database_initialized = false;
                            logging_out = true;
                            *LOGGED_IN.lock().await = false;

                            if logout_cleanup.is_none() {
                                let data_path = app
                                    .path()
                                    .app_data_dir()
                                    .map(|path| path.join("data"))
                                    .map_err(|error| {
                                        tracing::warn!(
                                            ?error,
                                            "Unable to resolve cached email data path during logout"
                                        );
                                    })
                                    .ok();
                                let cleanup_span = tracing::info_span!("worker.imap.logout.cleanup");
                                logout_cleanup = Some(tokio::spawn(
                                    cleanup_logout_resources(
                                        cleanup_pool,
                                        data_path,
                                        primary_session,
                                        background_session,
                                    )
                                    .instrument(cleanup_span),
                                ));
                            } else {
                                tracing::debug!("Logout cleanup is already running");
                            }
                        }
                        ImapCommand::FetchAttachment {
                            mail_box,
                            uid,
                            part_id,
                            response_channel,
                            username,
                            keychain_manager,
                            cancel_token,
                        } => {
                            let request = async {
                                let session = initialized_session.clone().ok_or_else(|| {
                                    Error::Other(
                                        "FetchAttachment failed: IMAP session is not initialized"
                                            .to_string(),
                                    )
                                })?;
                                match fetch_attachment_on_server(
                                    session,
                                    &mail_box,
                                    uid,
                                    &part_id,
                                )
                                .await
                                {
                                    Err(error) if is_imap_connection_error(&error) => {
                                        let new_session = reconnect_imap_session(
                                            &username,
                                            &keychain_manager,
                                            imap_endpoint.as_ref(),
                                        )
                                        .await?;
                                        initialized_session = Some(new_session.clone());
                                        fetch_attachment_on_server(
                                            new_session,
                                            &mail_box,
                                            uid,
                                            &part_id,
                                        )
                                        .await
                                    }
                                    result => result,
                                }
                            };
                            let result = tokio::select! {
                                result = time::timeout(Duration::from_secs(30), request) => result
                                    .map_err(|_| "Attachment request timed out".to_string())
                                    .and_then(|result| result.map_err(|error| error.to_string())),
                                _ = cancel_token.cancelled() => {
                                    Err("Attachment request cancelled".to_string())
                                }
                            };
                            if let Err(undelivered) = response_channel.send(result) {
                                tracing::error!(?undelivered, "Failed sending attachment result");
                            }
                        }
                        ImapCommand::SetEmailRead {
                            mail_box,
                            provider,
                            uid,
                            read,
                            response_channel,
                            username,
                            keychain_manager,
                        } => {
                            let result = async {
                                let session = initialized_session
                                    .clone()
                                    .ok_or_else(|| Error::Other(
                                        "SetEmailRead failed: IMAP session is not initialized".to_string(),
                                    ))?;
                                let pool_ref = pool.as_ref().ok_or_else(|| Error::Other(
                                    "SetEmailRead failed: sqlite pool is not initialized".to_string(),
                                ))?;

                                let server_result = set_email_read_on_server(
                                    session,
                                    &mail_box,
                                    uid,
                                    read,
                                )
                                .await;

                                match server_result {
                                    Err(error) if is_imap_connection_error(&error) => {
                                        tracing::warn!(
                                            error = %error,
                                            mailbox = mail_box.as_ref(),
                                            uid,
                                            "Unable to synchronize read state; reconnecting IMAP session"
                                        );
                                        let new_session = reconnect_imap_session(
                                            &username,
                                            &keychain_manager,
                                            imap_endpoint.as_ref(),
                                        )
                                        .await?;
                                        initialized_session = Some(new_session.clone());
                                        set_email_read_on_server(
                                            new_session,
                                            &mail_box,
                                            uid,
                                            read,
                                        )
                                        .await?;
                                    }
                                    result => result?,
                                }

                                let table_name = format!(
                                    "{}_{}",
                                    provider.as_ref(),
                                    mail_box.as_ref()
                                );
                                set_email_read(
                                    pool_ref,
                                    &table_name,
                                    uid,
                                    read,
                                )
                                .await?;

                                Ok::<(), Error>(())
                            }
                            .await
                            .map_err(|error| {
                                tracing::warn!(
                                    error = %error,
                                    mailbox = mail_box.as_ref(),
                                    uid,
                                    read,
                                    "Failed to synchronize email read state"
                                );
                                error.to_string()
                            });

                            if let Err(undelivered) = response_channel.send(result) {
                                tracing::error!(?undelivered, "Failed sending read-state result");
                            }
                        }
                        ImapCommand::RefreshEmails{mail_box, provider, response_channel: login_result_tx, username, keychain_manager} => {
                            let Some(session) = initialized_session.clone() else {
                                let message = "RefreshEmails failed: IMAP session is not initialized";
                                tracing::warn!("{message}");
                                let _ = login_result_tx.send(Err(message.to_string()));
                                continue;
                            };

                            let Some(pool_ref) = pool.as_ref() else {
                                let message = "RefreshEmails failed: sqlite pool is not initialized";
                                tracing::warn!("{message}");
                                let _ = login_result_tx.send(Err(message.to_string()));
                                continue;
                            };

                            let refresh_token = {
                                FETCH_MANAGER.refresh_token.lock().await.child_token()
                            };

                            let refresh_result = fetch_latest(
                                app.clone(),
                                session,
                                pool_ref,
                                &mail_box,
                                &provider,
                                refresh_token,
                            )
                            .await;

                            let refresh_result = match refresh_result {
                                Err(err) if is_imap_connection_error(&err) => {
                                    tracing::warn!(error = %err, "Unable to fetch latest email; reconnecting IMAP session");

                                    match reconnect_imap_session(
                                        &username,
                                        &keychain_manager,
                                        imap_endpoint.as_ref(),
                                    )
                                    .await
                                    {
                                        Ok(new_session) => {
                                            initialized_session = Some(new_session.clone());

                                            let retry_refresh_token = FETCH_MANAGER
                                                .refresh_token
                                                .lock()
                                                .await
                                                .child_token();

                                            fetch_latest(
                                                app.clone(),
                                                new_session,
                                                pool_ref,
                                                &mail_box,
                                                &provider,
                                                retry_refresh_token,
                                            )
                                            .await
                                        }
                                        Err(retry_err) => Err(retry_err),
                                    }
                                }
                                result => result,
                            };

                            let response = refresh_result
                                .map(|fetched| RefreshSummary {
                                    new_emails_count: fetched.count(),
                                    total_emails: fetched.total_emails(),
                                })
                                .map_err(|err| {
                                    tracing::warn!(error = %err, "Unable to fetch latest email");
                                    err.to_string()
                                });

                            if let Err(undelivered) = login_result_tx.send(response) {
                                tracing::error!(?undelivered, "Failed sending refresh result");
                            }
                        }
                    }
                }
                _ =  ticker.tick(), if database_initialized && !logging_out => {
                    let Some(mut session) = initialized_session.clone() else {
                        tracing::warn!("FetchEmails ignored: IMAP session is not initialized");
                        continue;
                    };

                    let Some(pool_ref) = pool.as_ref() else {
                        tracing::warn!("FetchEmails ignored: sqlite pool is not initialized");
                        continue;
                    };

                    let refresh_token = {
                        FETCH_MANAGER.refresh_token.lock().await.child_token()
                    };

                    for provider in &providers {
                        // Inbox polling is the user-facing path and must not depend on
                        // the optional second connection used to warm Sent and Spam.
                        match refresh_periodic_mailbox(
                            &app,
                            session.clone(),
                            pool_ref,
                            &MailBox::Inbox,
                            provider,
                            refresh_token.clone(),
                            imap_endpoint.as_ref(),
                        )
                        .await
                        {
                            Ok(Some(new_session)) => {
                                session = new_session.clone();
                                initialized_session = Some(new_session);
                            }
                            Ok(None) => {}
                            Err(err) => {
                                tracing::error!(error = ?err, "Failed inbox periodic refresh");
                            }
                        }

                        let session_background = match initialized_session_background.clone() {
                            Some(session) => session,
                            None => match reconnect_periodic_session(&app, imap_endpoint.as_ref()).await {
                                Ok(session) => {
                                    initialized_session_background = Some(session.clone());
                                    session
                                }
                                Err(err) => {
                                    tracing::warn!(
                                        error = ?err,
                                        "Background IMAP connection is unavailable; Inbox polling remains active"
                                    );
                                    continue;
                                }
                            },
                        };

                        match refresh_periodic_mailbox(
                            &app,
                            session_background.clone(),
                            pool_ref,
                            &MailBox::Sent,
                            provider,
                            refresh_token.clone(),
                            imap_endpoint.as_ref(),
                        )
                        .await
                        {
                            Ok(Some(new_session)) => {
                                initialized_session_background = Some(new_session);
                            }
                            Ok(None) => {}
                            Err(err) => {
                                tracing::error!(error = ?err, "Failed sent periodic refresh");
                            }
                        }

                        // Reuse the Sent connection after Inbox refresh finishes so messages
                        // moved by the Inbox spam rater are visible in the local Spam cache.
                        let spam_session = initialized_session_background
                            .clone()
                            .unwrap_or(session_background);
                        match refresh_periodic_mailbox(
                            &app,
                            spam_session,
                            pool_ref,
                            &MailBox::Spam,
                            provider,
                            refresh_token.clone(),
                            imap_endpoint.as_ref(),
                        )
                        .await
                        {
                            Ok(Some(new_session)) => {
                                initialized_session_background = Some(new_session);
                            }
                            Ok(None) => {}
                            Err(err) => {
                                tracing::error!(error = ?err, "Failed spam periodic refresh");
                            }
                        }
                    }
                }
            }
        }

        tracing::info!("IMAP worker task stopped");
        }
        .instrument(worker_span),
    );
}

#[tracing::instrument(
    name = "worker.imap.login",
    skip_all,
    fields(imap.host = config.imap_server_url.as_str(), imap.port = config.imap_port)
)]
async fn login(
    mut config: ImapClientConfig,
    initialized_session: &mut Option<SharedImapSession>,
    imap_client: async_imap::Client<TlsStream<TcpStream>>,
    pool: &mut Option<SqlitePool>,
) {
    match auth::login(&config.username, &config.password, Some(imap_client)).await {
        Ok(imap_session) => {
            *initialized_session = Some(Arc::new(tokio::sync::Mutex::new(imap_session)));
            *pool = Some(config.sqlite_pool);

            if let Some(tx) = config.login_result_tx.take() {
                let _ = tx.send(Ok(()));
            }
        }
        Err(err) => {
            tracing::error!(error = ?err, "IMAP login failed");

            if let Some(tx) = config.login_result_tx.take() {
                let _ = tx.send(Err(friendly_login_error_message("login", &err.to_string())));
            }
        }
    }
}
