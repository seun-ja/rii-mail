use async_imap::Session;
use async_native_tls::TlsStream;
use sqlx::SqlitePool;
use tokio::{net::TcpStream, sync::mpsc::UnboundedReceiver};

use crate::auth;
use crate::config::ImapClientConfig;
use crate::imap::{init_imap_client, ImapCommand};

fn friendly_login_error_message(stage: &str, raw: &str) -> String {
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

#[tracing::instrument(name = "background.get.emails", skip(session, pool))]
async fn get_emails(session: &mut Session<TlsStream<TcpStream>>, pool: &SqlitePool, size: u32) {
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

pub fn session_thread(
    mut imap_client_channel_rx: UnboundedReceiver<ImapClientConfig>,
    mut imap_cmd_channel_rx: UnboundedReceiver<ImapCommand>,
) {
    tokio::spawn(async move {
        let mut initialized_session: Option<async_imap::Session<TlsStream<TcpStream>>> = None;
        let mut pool: Option<SqlitePool> = None;

        while initialized_session.is_none() {
            let Some(config) = imap_client_channel_rx.recv().await else {
                ::tracing::warn!("IMAP config channel closed before login succeeded");
                break;
            };

            let mut login_result_tx = config.login_result_tx;

            match init_imap_client(&config.imap_server, config.imap_port).await {
                Ok(imap_client) => {
                    match auth::login(&config.username, &config.password, imap_client).await {
                        Ok(imap_session) => {
                            initialized_session = Some(imap_session);
                            pool = Some(config.sqlite_pool);
                            ::tracing::info!("IMAP session initialized");

                            if let Some(tx) = login_result_tx.take() {
                                let _ = tx.send(Ok(()));
                            }
                        }
                        Err(err) => {
                            ::tracing::error!(error = ?err, "IMAP login failed");

                            if let Some(tx) = login_result_tx.take() {
                                let _ = tx.send(Err(friendly_login_error_message(
                                    "login",
                                    &err.to_string(),
                                )));
                            }
                        }
                    }
                }
                Err(err) => {
                    ::tracing::error!(error = ?err, "Failed to initialize IMAP client");

                    if let Some(tx) = login_result_tx.take() {
                        let _ =
                            tx.send(Err(friendly_login_error_message("init", &err.to_string())));
                    }
                }
            }
        }

        while let Some(cmd) = imap_cmd_channel_rx.recv().await {
            match cmd {
                ImapCommand::FetchEmails(size) => {
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
                ImapCommand::Logout => {
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
            }
        }

        ::tracing::info!("IMAP worker task stopped");
    });
}
