use std::{path::PathBuf, sync::Arc};

use crate::{
    auth::AppleKeychainManager,
    db::Provider,
    error::{Error, ErrorMessage},
    llm::Email,
};
use lettre::{transport::smtp::authentication::Credentials, SmtpTransport};
use llm::Agent;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::fs;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum InitStatus {
    /// Configuration file doesn't exist - user needs to complete full setup
    Setup,
    /// Configuration exists but user is not authenticated - show login page
    ///
    /// In case where there's a redirect because of error, there would be `Some`
    Login(Option<ErrorMessage>),
    /// User is fully authenticated and app is ready
    SignedIn,
    /// User is returning and already authenticated
    ReturningSigned,
}

#[derive(Clone)]
pub struct Account {
    pub name: Option<String>,
    pub email: String,
    pub provider: Provider,
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct ChatGptConfig {
    #[serde(default, rename = "chatgpt_client_id")]
    pub client_id: Option<String>,
    #[serde(default, rename = "chatgpt_id_token")]
    pub id_token: Option<String>,
    #[serde(default, rename = "chatgpt_email")]
    pub email: Option<String>,
    #[serde(default, rename = "chatgpt_subject")]
    pub subject: Option<String>,
    #[serde(default, rename = "chatgpt_scopes")]
    pub scopes: Vec<String>,
    #[serde(default, rename = "chatgpt_token_expires_at")]
    pub token_expires_at: Option<u64>,
    #[serde(default, rename = "chatgpt_host_id")]
    pub host_id: Option<String>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Config {
    pub imap_server_url: String,
    pub smtp_relay_url: String,
    pub imap_port: u16,
    pub sqlite_db: String,
    pub accounts: Vec<String>,
    #[serde(default)]
    pub llm_provider: Option<String>,
    pub default_model: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub access_token: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(flatten)]
    pub chatgpt: ChatGptConfig,
}

impl Config {
    #[tracing::instrument(name = "config.load", skip_all, err)]
    pub async fn init(path: &PathBuf) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let config_json = fs::read_to_string(path).await?;
        let config: Self = serde_json::from_str(&config_json)?;

        if config.imap_port == 0 || config.imap_server_url.is_empty() {
            return Err("IMAP Port and Server not set".into());
        }

        Ok(config)
    }

    /// Saves configuration with owner-only permissions because it may contain
    /// renewable OAuth credentials.
    pub async fn save(
        &self,
        path: &PathBuf,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        fs::write(path, serde_json::to_string_pretty(self)?).await?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await?;
        }

        Ok(())
    }
}

pub enum AppState {
    Initialized(Arc<InitializedState>),
    Fresh,
}

/// Coordinates startup status requests so the Keychain is read at most once
/// while the app is determining whether a returning user is signed in.
pub struct StartupStatusCache(pub tokio::sync::Mutex<Option<InitStatus>>);

impl AppState {
    pub fn state(&self) -> &InitializedState {
        match self {
            AppState::Initialized(state) => state,
            AppState::Fresh => panic!("Application not initialized"),
        }
    }
}

pub struct InitializedState {
    pub local_inference: Agent<Email>,
    pub sqlite_pool: SqlitePool,
    pub apple_keychain_manager: AppleKeychainManager,
    pub smtp_transport_client: SmtpTransport,
    pub llm_agent: tokio::sync::RwLock<Option<Agent<Email>>>,
    pub accounts: Vec<Account>,
}

#[tracing::instrument(
    name = "smtp.transport.initialize",
    skip_all,
    fields(smtp.host = smtp_server_url),
    err
)]
pub fn smtp_transport_client(
    smtp_server_url: &str,
    creds: Credentials,
) -> Result<SmtpTransport, Error> {
    let transport = SmtpTransport::relay(smtp_server_url)?
        .credentials(creds)
        .build();

    Ok(transport)
}

pub struct ImapClientConfig {
    pub username: String,
    pub password: String,
    pub imap_server_url: String,
    pub imap_port: u16,
    pub sqlite_pool: SqlitePool,
    pub login_result_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
}
