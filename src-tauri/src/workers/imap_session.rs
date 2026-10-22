use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use async_native_tls::TlsStream;
use sqlx::SqlitePool;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot::{self, Sender};
use tokio::sync::Mutex;
use tokio::time::{self, MissedTickBehavior};
use tokio::{net::TcpStream, sync::mpsc::UnboundedReceiver};
use tracing::Instrument as _;

use crate::config::ImapClientConfig;
use crate::db::{MailBox, Provider};
use crate::email_cache::SharedImapSession;
use crate::error::{friendly_login_error_message, Error};
use crate::imap::{init_imap_client, ImapCommand, RefreshSummary};
use crate::{auth, FETCH_MANAGER, LOGGED_IN, LOGGING_IN};

pub(crate) fn is_imap_connection_error(error: &Error) -> bool {
    matches!(
        error,
        Error::Imap(async_imap::error::Error::Io(_) | async_imap::error::Error::ConnectionLost)
    )
}

pub async fn worker(
    mut imap_client_channel_rx: UnboundedReceiver<ImapClientConfig>,
    mut imap_client_returning_user_channel_rx: UnboundedReceiver<ImapClientConfig>,
    mut imap_cmd_channel_rx: UnboundedReceiver<ImapCommand>,
    logout_result_tx: UnboundedSender<()>,
    inbox_intial_email_populated_tx: Arc<Mutex<Option<Sender<()>>>>,
    sent_intial_email_populated_tx: Arc<Mutex<Option<Sender<()>>>>,
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

        let (_email_cmd_tx, _email_cmd_rx) = oneshot::channel::<()>();

        // 1. Create an interval timer (e.g., every 3 seconds)
        let mut ticker = time::interval(Duration::from_secs(60));

        // Optional: Prevents bursts of ticks if your other code runs slow
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

        while imap_client_channel_open || imap_cmd_channel_open || logged_in_finalised {
            tokio::select! {
                maybe_config = imap_client_channel_rx.recv(), if imap_client_channel_open => {
                    let Some(mut config) = maybe_config else {
                        imap_client_channel_open = false;
                        tracing::warn!("IMAP config channel closed");
                        continue;
                    };

                    let background_config = ImapClientConfig {
                        username: config.username.clone(),
                        password: config.password.clone(),
                        imap_server_url: config.imap_server_url.clone(),
                        imap_port: config.imap_port,
                        sqlite_pool: config.sqlite_pool.clone(),
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

                    let background_config = ImapClientConfig {
                        username: config.username.clone(),
                        password: config.password.clone(),
                        imap_server_url: config.imap_server_url.clone(),
                        imap_port: config.imap_port,
                        sqlite_pool: config.sqlite_pool.clone(),
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

                            let Some(session_background) = initialized_session_background.as_ref() else {
                                tracing::warn!("FetchEmails ignored: background IMAP session is not initialized");
                                continue;
                            };

                            let Some(pool_ref) = pool.as_ref() else {
                                tracing::warn!("FetchEmails ignored: sqlite pool is not initialized");
                                continue;
                            };

                            let inbox_tx = inbox_intial_email_populated_tx.clone();
                            let sent_tx = sent_intial_email_populated_tx.clone();

                            match crate::email_cache::fetch_emails(
                                session.clone(),
                                Some(session_background.clone()),
                                pool_ref,
                                provider.clone(),
                                cancel_token,
                                inbox_tx,
                                sent_tx,
                            )
                            .await
                            {
                                Ok(_) => {
                                    tracing::info!("Initial sync completed");
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
                            if let Some(session) = initialized_session.take() {
                                let mut session = session.lock().await;
                                if let Err(err) = session.logout().await {
                                    tracing::warn!(error = ?err, "Failed to logout IMAP session");
                                }
                            } else {
                                tracing::info!("Logout ignored: IMAP session is not initialized");
                            }

                            if let Some(session_background) = initialized_session_background.take() {
                                let mut session_background = session_background.lock().await;
                                if let Err(err) = session_background.logout().await {
                                    tracing::warn!(error = ?err, "Failed to logout background IMAP session");
                                }
                            } else {
                                tracing::info!("Logout ignored: background IMAP session is not initialized");
                            }

                            pool = None;
                            imap_endpoint = None;
                            providers.clear();
                            initial_fetch_completed.clear();
                            database_initialized = false;
                            logging_out = true;

                            *LOGGED_IN.lock().await = false;
                            let _ = logout_result_tx.send(());
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

                            let refresh_result = crate::email_cache::fetch_latest(
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

                                    match keychain_manager.retrieve_password(&username) {
                                        Ok(password) => {
                                            let reconnect_result = match imap_endpoint.as_ref() {
                                                Some((server, port)) => {
                                                    match init_imap_client(server, *port).await {
                                                        Ok(client) => auth::login(
                                                            &username,
                                                            &password,
                                                            Some(client),
                                                        )
                                                        .await,
                                                        Err(reconnect_err) => Err(reconnect_err),
                                                    }
                                                }
                                                None => Err(Error::Other(
                                                    "IMAP endpoint is not initialized".to_string(),
                                                )),
                                            };

                                            match reconnect_result {
                                                Ok(new_session) => {
                                                    let new_session = Arc::new(Mutex::new(new_session));
                                                    initialized_session = Some(new_session.clone());

                                                    let retry_refresh_token = FETCH_MANAGER
                                                        .refresh_token
                                                        .lock()
                                                        .await
                                                        .child_token();

                                                    crate::email_cache::fetch_latest(
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
                                        Err(keychain_err) => Err(keychain_err),
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
                    let Some(session) = initialized_session.as_ref() else {
                        tracing::warn!("FetchEmails ignored: IMAP session is not initialized");
                        continue;
                    };

                    let Some(session_background) = initialized_session_background.as_ref() else {
                        tracing::warn!("FetchEmails ignored: background IMAP session is not initialized");
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
                        let provider_for_inbox = provider.clone();
                        let provider_for_sent = provider.clone();

                        let (inbox_result, sent_result) = tokio::join!(
                            crate::email_cache::fetch_latest(
                                session.clone(),
                                pool_ref,
                                &MailBox::Inbox,
                                &provider_for_inbox,
                                refresh_token.clone()
                            ),
                            crate::email_cache::fetch_latest(
                                session_background.clone(),
                                pool_ref,
                                &MailBox::Sent,
                                &provider_for_sent,
                                refresh_token.clone()
                            )
                        );

                        if let Err(err) = inbox_result {
                            tracing::error!(error = ?err, "Failed inbox periodic refresh");
                        }

                        if let Err(err) = sent_result {
                            tracing::error!(error = ?err, "Failed sent periodic refresh");
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
