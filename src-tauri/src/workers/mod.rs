mod email_fetcher;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use async_native_tls::TlsStream;
use sqlx::SqlitePool;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot::{self};
use tokio::time::{self, MissedTickBehavior};
use tokio::{net::TcpStream, sync::mpsc::UnboundedReceiver};

use crate::auth;
use crate::config::ImapClientConfig;
use crate::db::{MailBox, Providers};
use crate::email_cache::SharedImapSession;
use crate::imap::{init_imap_client, ImapCommand};

pub(crate) fn friendly_login_error_message(stage: &str, raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();

    if lower.contains("authentication")
        || lower.contains("invalid credentials")
        || lower.contains("login failed")
        || lower.contains("auth")
    {
        return "Invalid email or password. Please verify your credentials and try again."
            .to_string();
    }

    if lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("connection")
        || lower.contains("dns")
        || lower.contains("network")
        || lower.contains("refused")
    {
        return "Unable to reach the mail server. Check your internet connection and IMAP server settings."
            .to_string();
    }

    if lower.contains("tls") || lower.contains("certificate") || lower.contains("ssl") {
        return "Secure connection to the mail server failed. Please verify TLS/SSL settings."
            .to_string();
    }

    match stage {
        "init" => {
            "Could not connect to the IMAP server. Please check server host and port.".to_string()
        }
        "login" => "Login failed due to a server error. Please try again shortly.".to_string(),
        _ => "Login failed. Please try again.".to_string(),
    }
}

pub(crate) struct _FetchEmailsCmd {
    pub mail_box: MailBox,
    pub provider: Providers,
}

pub fn session_thread(
    mut imap_client_channel_rx: UnboundedReceiver<ImapClientConfig>,
    mut imap_client_returning_user_channel_rx: UnboundedReceiver<ImapClientConfig>,
    mut imap_cmd_channel_rx: UnboundedReceiver<ImapCommand>,
    logout_result_tx: UnboundedSender<()>,
) {
    tokio::spawn(async move {
        let mut initialized_session: Option<SharedImapSession> = None;
        let mut initialized_session_background: Option<SharedImapSession> = None;

        let mut pool: Option<SqlitePool> = None;
        let mut imap_client_channel_open = true;
        let mut imap_cmd_channel_open = true;
        let mut providers: HashSet<Providers> = HashSet::new();
        let mut database_initialized = false;

        let (_email_cmd_tx, _email_cmd_rx) = oneshot::channel::<()>();

        // 1. Create an interval timer (e.g., every 3 seconds)
        let mut ticker = time::interval(Duration::from_secs(60));

        // Optional: Prevents bursts of ticks if your other code runs slow
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

        while imap_client_channel_open || imap_cmd_channel_open {
            tokio::select! {
                maybe_config = imap_client_channel_rx.recv(), if imap_client_channel_open => {
                    let Some(mut config) = maybe_config else {
                        imap_client_channel_open = false;
                        ::tracing::warn!("IMAP config channel closed");
                        continue;
                    };

                    let background_config = ImapClientConfig {
                        username: config.username.clone(),
                        password: config.password.clone(),
                        imap_server: config.imap_server.clone(),
                        imap_port: config.imap_port,
                        sqlite_pool: config.sqlite_pool.clone(),
                        login_result_tx: None,
                    };

                    match init_imap_client(&config.imap_server, config.imap_port).await {
                        Ok(imap_client) => {
                            login(config, &mut initialized_session, imap_client, &mut pool).await;
                        }
                        Err(err) => {
                            ::tracing::error!(error = ?err, "Failed to initialize IMAP client");

                            if let Some(tx) = config.login_result_tx.take() {
                                let _ = tx.send(Err(friendly_login_error_message("init", &err.to_string())));
                            }
                            continue;
                        }
                    }

                    match init_imap_client(&background_config.imap_server, background_config.imap_port).await {
                        Ok(imap_client) => {
                            login(
                                background_config,
                                &mut initialized_session_background,
                                imap_client,
                                &mut pool,
                            )
                            .await;

                            ::tracing::info!("IMAP sessions initialized");
                        }
                        Err(err) => {
                            ::tracing::error!(error = ?err, "Failed to initialize background IMAP client");
                        }
                    }
                }
                maybe_returning_user_config = imap_client_returning_user_channel_rx.recv() => {
                    ::tracing::info!("Received IMAP config for returning user");
                    let Some(mut config) = maybe_returning_user_config else {
                        ::tracing::warn!("Returning user IMAP config channel closed");
                        continue;
                    };

                    let background_config = ImapClientConfig {
                        username: config.username.clone(),
                        password: config.password.clone(),
                        imap_server: config.imap_server.clone(),
                        imap_port: config.imap_port,
                        sqlite_pool: config.sqlite_pool.clone(),
                        login_result_tx: None,
                    };


                    match init_imap_client(&config.imap_server, config.imap_port).await {
                        Ok(imap_client) => {
                            login(config, &mut initialized_session, imap_client, &mut pool).await;
                        }
                        Err(err) => {
                            ::tracing::error!(error = ?err, "Failed to initialize IMAP client for returning user");

                            if let Some(tx) = config.login_result_tx.take() {
                                let _ = tx.send(Err(friendly_login_error_message("init", &err.to_string())));
                            }
                            continue;
                        }
                    }

                    match init_imap_client(&background_config.imap_server, background_config.imap_port).await {
                        Ok(imap_client) => {
                            login(
                                background_config,
                                &mut initialized_session_background,
                                imap_client,
                                &mut pool,
                            )
                            .await;

                            ::tracing::info!("IMAP sessions initialized for returning user");
                        }
                        Err(err) => {
                            ::tracing::error!(error = ?err, "Failed to initialize background IMAP client for returning user");
                        }
                    }
                }
                maybe_cmd = imap_cmd_channel_rx.recv(), if imap_cmd_channel_open => {
                    let Some(cmd) = maybe_cmd else {
                        imap_cmd_channel_open = false;
                        continue;
                    };

                    match cmd {
                        ImapCommand::FetchEmails(mail_box, provider) => {
                            let Some(session) = initialized_session.as_ref() else {
                                ::tracing::warn!("FetchEmails ignored: IMAP session is not initialized");
                                continue;
                            };

                            let Some(session_background) = initialized_session_background.as_ref() else {
                                ::tracing::warn!("FetchEmails ignored: background IMAP session is not initialized");
                                continue;
                            };

                            let Some(pool_ref) = pool.as_ref() else {
                                ::tracing::warn!("FetchEmails ignored: sqlite pool is not initialized");
                                continue;
                            };

                            if let Err(err) = crate::email_cache::fetch_emails(
                                session.clone(),
                                Some(session_background.clone()),
                                pool_ref,
                                mail_box,
                                provider.clone(),
                            )
                            .await
                            {
                                ::tracing::error!(error = ?err, "Failed initial mailbox fetch");
                            }

                            // if let Ok(state) = update_state_rx.await {
                            //     ::tracing::info!(state, "FetchEmails progress update");
                            //     let _ = crate::email_cache::fetch_emails(session, pool_ref, &MailBox::Sent, &provider, None).await;
                            // }

                            database_initialized = true;

                            providers.insert(provider);
                        }
                        ImapCommand::Logout => {
                            if let Some(session) = initialized_session.take() {
                                let mut session = session.lock().await;
                                if let Err(err) = session.logout().await {
                                    ::tracing::warn!(error = ?err, "Failed to logout IMAP session");
                                }
                            } else {
                                ::tracing::info!("Logout ignored: IMAP session is not initialized");
                            }

                            if let Some(session_background) = initialized_session_background.take() {
                                let mut session_background = session_background.lock().await;
                                if let Err(err) = session_background.logout().await {
                                    ::tracing::warn!(error = ?err, "Failed to logout background IMAP session");
                                }
                            } else {
                                ::tracing::info!("Logout ignored: background IMAP session is not initialized");
                            }

                            let _ = logout_result_tx.send(());
                            pool = None;
                            providers.clear();
                            database_initialized = false;
                        }
                        ImapCommand::RefreshEmails(mail_box, provider, login_result_tx) => {
                            let Some(session) = initialized_session.as_ref() else {
                                ::tracing::warn!("FetchEmails ignored: IMAP session is not initialized");
                                continue;
                            };

                            let Some(pool_ref) = pool.as_ref() else {
                                ::tracing::warn!("FetchEmails ignored: sqlite pool is not initialized");
                                continue;
                            };

                            if let Ok(fetched_count) = crate::email_cache::fetch_latest(
                                session.clone(),
                                pool_ref,
                                mail_box,
                                provider,
                            )
                            .await
                            {
                                let _ = login_result_tx.send(Ok(fetched_count.count()));
                            }

                            // TODO send the fetched emails count to the other thread so that we can trigger a frontend refresh if new emails were found
                        }
                    }
                }
                _ =  ticker.tick(), if database_initialized => {
                    let Some(session) = initialized_session.as_ref() else {
                        ::tracing::warn!("FetchEmails ignored: IMAP session is not initialized");
                        continue;
                    };

                    let Some(session_background) = initialized_session_background.as_ref() else {
                        ::tracing::warn!("FetchEmails ignored: background IMAP session is not initialized");
                        continue;
                    };

                    let Some(pool_ref) = pool.as_ref() else {
                        ::tracing::warn!("FetchEmails ignored: sqlite pool is not initialized");
                        continue;
                    };

                    for provider in &providers {
                        let provider_for_inbox = provider.clone();
                        let provider_for_sent = provider.clone();

                        let (inbox_result, sent_result) = tokio::join!(
                            crate::email_cache::fetch_latest(
                                session.clone(),
                                pool_ref,
                                MailBox::Inbox,
                                provider_for_inbox,
                            ),
                            crate::email_cache::fetch_latest(
                                session_background.clone(),
                                pool_ref,
                                MailBox::Sent,
                                provider_for_sent,
                            )
                        );

                        if let Err(err) = inbox_result {
                            ::tracing::error!(error = ?err, "Failed inbox periodic refresh");
                        }

                        if let Err(err) = sent_result {
                            ::tracing::error!(error = ?err, "Failed sent periodic refresh");
                        }
                    }
                }
            }
        }

        ::tracing::info!("IMAP worker task stopped");
    });
}

async fn login(
    mut config: ImapClientConfig,
    initialized_session: &mut Option<SharedImapSession>,
    imap_client: async_imap::Client<TlsStream<TcpStream>>,
    pool: &mut Option<SqlitePool>,
) {
    match auth::login(&config.username, &config.password, imap_client).await {
        Ok(imap_session) => {
            *initialized_session = Some(Arc::new(tokio::sync::Mutex::new(imap_session)));
            *pool = Some(config.sqlite_pool);

            if let Some(tx) = config.login_result_tx.take() {
                let _ = tx.send(Ok(()));
            }
        }
        Err(err) => {
            ::tracing::error!(error = ?err, "IMAP login failed");

            if let Some(tx) = config.login_result_tx.take() {
                let _ = tx.send(Err(friendly_login_error_message("login", &err.to_string())));
            }
        }
    }
}
