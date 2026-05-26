use async_imap::Session;
use async_native_tls::TlsStream;
use sqlx::SqlitePool;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot::Sender;
use tokio::{net::TcpStream, sync::mpsc::UnboundedReceiver};

use crate::auth;
use crate::config::{ImapClientConfig, ReturningUserImapClientConfig};
use crate::db::MailBox;
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
async fn get_emails(
    session: &mut Session<TlsStream<TcpStream>>,
    pool: &SqlitePool,
    size: u32,
    mailbox: &MailBox,
    provider: &crate::db::Providers,
) {
    let mut attempt = 0;
    while attempt < 3 {
        if let Err(e) =
            crate::email_cache::fetch_emails(session, size, pool, mailbox, provider).await
        {
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
    mut imap_client_returning_user_channel_rx: UnboundedReceiver<ReturningUserImapClientConfig>,
    mut imap_cmd_channel_rx: UnboundedReceiver<ImapCommand>,
    logout_result_tx: UnboundedSender<()>,
) {
    tokio::spawn(async move {
        let mut initialized_session: Option<async_imap::Session<TlsStream<TcpStream>>> = None;
        let mut pool: Option<SqlitePool> = None;
        let mut imap_client_channel_open = true;
        let mut imap_cmd_channel_open = true;

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
                        ImapCommand::FetchEmails(size, mail_box, provider) => {
                            let Some(session) = initialized_session.as_mut() else {
                                ::tracing::warn!("FetchEmails ignored: IMAP session is not initialized");
                                continue;
                            };

                            let Some(pool_ref) = pool.as_ref() else {
                                ::tracing::warn!("FetchEmails ignored: sqlite pool is not initialized");
                                continue;
                            };

                            get_emails(session, pool_ref, size, &mail_box, &provider).await;
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
