use async_imap::Session;
use async_native_tls::TlsStream;
use sqlx::SqlitePool;
use tokio::{net::TcpStream, sync::mpsc::UnboundedReceiver};

use crate::auth;
use crate::config::ImapClientConfig;
use crate::imap::{init_imap_client, ImapCommand};

#[tracing::instrument(name = "background.get.emails", skip(session, pool))]
pub async fn get_emails(session: &mut Session<TlsStream<TcpStream>>, pool: &SqlitePool, size: u32) {
    let mut attempt = 0;
    while attempt < 3 {
        if let Err(e) = crate::email_cache::fetch_emails(session, size, pool).await {
            tracing::error!("Failed to fetch emails: {}", e);
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            attempt += 1;
        } else {
            break;
        }
    }
}

pub async fn session_thread(
    mut imap_client_channel_rx: UnboundedReceiver<ImapClientConfig>,
    mut imap_cmd_channel_rx: UnboundedReceiver<ImapCommand>,
) {
    tokio::spawn(async move {
        let mut initialized_session: Option<async_imap::Session<TlsStream<TcpStream>>> = None;
        let mut pool: Option<SqlitePool> = None;
        let mut config_rx_open = true;
        let mut cmd_rx_open = true;

        while config_rx_open || cmd_rx_open {
            tokio::select! {
                config = imap_client_channel_rx.recv(), if config_rx_open => {
                    match config {
                        Some(config) => {
                            match init_imap_client(&config.imap_server, config.imap_port).await {
                                Ok(imap_client) => {
                                    match auth::login(&config.username, &config.password, imap_client).await {
                                        Ok(imap_session) => {
                                            initialized_session = Some(imap_session);
                                            pool = Some(config.sqlite_pool);
                                            ::tracing::info!("IMAP session initialized");
                                        }
                                        Err(err) => {
                                            initialized_session = None;
                                            pool = None;
                                            ::tracing::error!(error = ?err, "IMAP login failed");
                                        }
                                    }
                                }
                                Err(err) => {
                                    initialized_session = None;
                                    pool = None;
                                    ::tracing::error!(error = ?err, "Failed to initialize IMAP client");
                                }
                            }
                        }
                        None => {
                            config_rx_open = false;
                            ::tracing::info!("IMAP config channel closed");
                        }
                    }
                }
                cmd = imap_cmd_channel_rx.recv(), if cmd_rx_open => {
                    match cmd {
                        Some(ImapCommand::FetchEmails(size)) => {
                            let Some(session) = initialized_session.as_mut() else {
                                ::tracing::warn!("FetchEmails ignored: IMAP session is not initialized");
                                continue;
                            };

                            let Some(pool_ref) = pool.as_ref() else {
                                ::tracing::warn!("FetchEmails ignored: sqlite pool is not initialized");
                                continue;
                            };

                            get_emails(session, pool_ref, size).await;
                        }
                        Some(ImapCommand::Logout) => {
                            if let Some(session) = initialized_session.as_mut() {
                                if let Err(err) = session.logout().await {
                                    ::tracing::warn!(error = ?err, "Failed to logout IMAP session");
                                }
                            } else {
                                ::tracing::info!("Logout ignored: IMAP session is not initialized");
                            }

                            initialized_session = None;
                            pool = None;
                        }
                        None => {
                            cmd_rx_open = false;
                            ::tracing::info!("IMAP command channel closed");
                        }
                    }
                }
            }
        }

        ::tracing::info!("IMAP worker task stopped");
    });
}
