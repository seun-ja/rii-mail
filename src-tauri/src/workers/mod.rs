use std::collections::HashSet;
use std::time::Duration;

use async_native_tls::TlsStream;
use sqlx::SqlitePool;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot::Sender;
use tokio::time::{self, MissedTickBehavior};
use tokio::{net::TcpStream, sync::mpsc::UnboundedReceiver};

use crate::auth;
use crate::config::{ImapClientConfig, ReturningUserImapClientConfig};
use crate::db::{MailBox, Providers};
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

pub fn session_thread(
    mut imap_client_channel_rx: UnboundedReceiver<ImapClientConfig>,
    mut imap_client_returning_user_channel_rx: UnboundedReceiver<ReturningUserImapClientConfig>,
    mut imap_cmd_channel_rx: UnboundedReceiver<ImapCommand>,
    logout_result_tx: UnboundedSender<()>,
) {
    tokio::spawn(async move {
        let mut initialized_session: Option<async_imap::Session<TlsStream<TcpStream>>> = None;
        let mut pool: Option<SqlitePool> = None;
        let mut imap_client_channel_open = true;
        let mut imap_cmd_channel_open = true;
        let mut providers: HashSet<Providers> = HashSet::new();

        // 1. Create an interval timer (e.g., every 3 seconds)
        let mut ticker = time::interval(Duration::from_secs(60));

        // Optional: Prevents bursts of ticks if your other code runs slow
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

        while imap_client_channel_open || imap_cmd_channel_open {
            tokio::select! {
                maybe_config = imap_client_channel_rx.recv(), if imap_client_channel_open => {
                    let Some(config) = maybe_config else {
                        imap_client_channel_open = false;
                        ::tracing::warn!("IMAP config channel closed");
                        continue;
                    };

                    let mut login_result_tx = config.login_result_tx;

                    match init_imap_client(&config.imap_server, config.imap_port).await {
                        Ok(imap_client) => {
                            login(&config.username, &config.password, &mut initialized_session, login_result_tx, imap_client, &mut pool, config.sqlite_pool).await;

                            ::tracing::info!("IMAP session initialized");
                        }
                        Err(err) => {
                            ::tracing::error!(error = ?err, "Failed to initialize IMAP client");

                            if let Some(tx) = login_result_tx.take() {
                                let _ = tx.send(Err(friendly_login_error_message("init", &err.to_string())));
                            }
                        }
                    }
                }
                maybe_returning_user_config = imap_client_returning_user_channel_rx.recv() => {
                    let Some(config) = maybe_returning_user_config else {
                        ::tracing::warn!("Returning user IMAP config channel closed");
                        continue;
                    };

                    let mut login_result_tx = config.login_result_tx;

                    match init_imap_client(&config.imap_server, config.imap_port).await {
                        Ok(imap_client) => {
                            let password = match config.apple_keychain_manager.retrieve_password(&config.username) {
                                Ok(pw) => pw,
                                Err(err) => {
                                    ::tracing::error!(error = ?err, "Failed to retrieve password from Apple Keychain for returning user");
                                    if let Some(tx) = login_result_tx.take() {
                                        let _ = tx.send(Err("Failed to retrieve credentials for returning user. Please log in again.".to_string()));
                                    }
                                    continue;
                                }
                            };

                            login(&config.username, &password, &mut initialized_session, login_result_tx, imap_client, &mut pool, config.sqlite_pool).await;

                            ::tracing::info!("IMAP session initialized for returning user");
                        }
                        Err(err) => {
                            ::tracing::error!(error = ?err, "Failed to initialize IMAP client for returning user");

                            if let Some(tx) = login_result_tx.take() {
                                let _ = tx.send(Err(friendly_login_error_message("init", &err.to_string())));
                            }
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
                            let Some(session) = initialized_session.as_mut() else {
                                ::tracing::warn!("FetchEmails ignored: IMAP session is not initialized");
                                continue;
                            };

                            let Some(pool_ref) = pool.as_ref() else {
                                ::tracing::warn!("FetchEmails ignored: sqlite pool is not initialized");
                                continue;
                            };

                            let _ = crate::email_cache::fetch_emails(session, pool_ref, &mail_box, &provider).await;
                            let _ = crate::email_cache::fetch_emails(session, pool_ref, &MailBox::Sent, &provider).await;

                            providers.insert(provider);
                        }
                        ImapCommand::Logout => {
                            if let Some(session) = initialized_session.as_mut() {
                                if let Err(err) = session.logout().await {
                                    ::tracing::warn!(error = ?err, "Failed to logout IMAP session");
                                }
                            } else {
                                ::tracing::info!("Logout ignored: IMAP session is not initialized");
                            }

                            let _ = logout_result_tx.send(());
                            initialized_session = None;
                            pool = None;
                        }
                        ImapCommand::RefreshEmails(mail_box, provider, rx) => {
                            let Some(session) = initialized_session.as_mut() else {
                                ::tracing::warn!("FetchEmails ignored: IMAP session is not initialized");
                                continue;
                            };

                            let Some(pool_ref) = pool.as_ref() else {
                                ::tracing::warn!("FetchEmails ignored: sqlite pool is not initialized");
                                continue;
                            };

                            if let Ok(fetched_count) = crate::email_cache::fetch_latest(session, pool_ref, &mail_box, &provider).await {
                                let _ = rx.send(Ok(fetched_count.count()));
                            }

                            // TODO send the fetched emails count to the other thread so that we can trigger a frontend refresh if new emails were found
                        }
                    }
                }
                _ =  ticker.tick() => {
                    let Some(session) = initialized_session.as_mut() else {
                        ::tracing::warn!("FetchEmails ignored: IMAP session is not initialized");
                        continue;
                    };

                    let Some(pool_ref) = pool.as_ref() else {
                        ::tracing::warn!("FetchEmails ignored: sqlite pool is not initialized");
                        continue;
                    };

                    let mail_box = [MailBox::Inbox, MailBox::Sent];

                    for boxx in mail_box {
                        for provider in &providers {
                            if let Err(err) = crate::email_cache::fetch_latest(session, pool_ref, &boxx, provider).await {
                                ::tracing::error!(error = ?err, "Failed to fetch latest emails for periodic refresh");
                            }
                        }
                    }
                }
            }
        }

        ::tracing::info!("IMAP worker task stopped");
    });
}

async fn login(
    username: &str,
    password: &str,
    initialized_session: &mut Option<async_imap::Session<TlsStream<TcpStream>>>,
    mut login_result_tx: Option<Sender<Result<(), String>>>,
    imap_client: async_imap::Client<TlsStream<TcpStream>>,
    pool: &mut Option<SqlitePool>,
    sqlite_pool: SqlitePool,
) {
    match auth::login(username, password, imap_client).await {
        Ok(imap_session) => {
            if let Some(existing_session) = initialized_session.as_mut() {
                if let Err(err) = existing_session.logout().await {
                    ::tracing::warn!(error = ?err, "Failed to logout previous IMAP session before re-login");
                }
            }

            *initialized_session = Some(imap_session);
            *pool = Some(sqlite_pool);

            if let Some(tx) = login_result_tx.take() {
                let _ = tx.send(Ok(()));
            }
        }
        Err(err) => {
            ::tracing::error!(error = ?err, "IMAP login failed");

            if let Some(tx) = login_result_tx.take() {
                let _ = tx.send(Err(friendly_login_error_message("login", &err.to_string())));
            }
        }
    }
}
