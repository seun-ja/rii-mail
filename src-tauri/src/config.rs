use std::{
    path::PathBuf,
    sync::{Arc, LazyLock},
};

use crate::{
    auth::AppleKeychainManager,
    db::Provider,
    error::{Error, ErrorMessage},
    llm::{Email, LocalInference},
};
use lettre::{transport::smtp::authentication::Credentials, SmtpTransport};
use llm::Agent;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::{fs, io::AsyncWriteExt as _};

static CONFIG_SAVE_LOCK: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_html: Option<String>,
    #[serde(
        default,
        flatten,
        deserialize_with = "deserialize_optional_chatgpt_config"
    )]
    pub chatgpt: Option<ChatGptConfig>,
}

fn deserialize_optional_chatgpt_config<'de, D>(
    deserializer: D,
) -> Result<Option<ChatGptConfig>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    let fields = value
        .as_object()
        .ok_or_else(|| serde::de::Error::custom("ChatGPT config must be an object"))?;
    let has_chatgpt_fields = [
        "chatgpt_client_id",
        "chatgpt_id_token",
        "chatgpt_email",
        "chatgpt_subject",
        "chatgpt_scopes",
        "chatgpt_token_expires_at",
        "chatgpt_host_id",
    ]
    .iter()
    .any(|field| fields.contains_key(*field));
    if !has_chatgpt_fields {
        return Ok(None);
    }

    serde_json::from_value(value)
        .map(Some)
        .map_err(serde::de::Error::custom)
}

impl Config {
    #[tracing::instrument(name = "config.load", skip_all, err)]
    pub async fn init(path: &PathBuf) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let config_json = fs::read_to_string(path).await?;
        let config: Self = serde_json::from_str(&config_json)?;

        if config.imap_port == 0 || config.imap_server_url.is_empty() {
            return Err("IMAP Port and Server not set".into());
        }

        if config.smtp_relay_url.trim().is_empty() {
            return Err("SMTP relay server not set".into());
        }

        tracing::info!(
            imap.host = %config.imap_server_url,
            smtp.host = %config.smtp_relay_url,
            config.path = %path.display(),
            "Loaded mail server configuration"
        );

        if config
            .smtp_relay_url
            .eq_ignore_ascii_case("smtp.mail.yahoo.com")
            && !config
                .imap_server_url
                .to_ascii_lowercase()
                .contains("yahoo")
        {
            tracing::warn!(
                imap.host = %config.imap_server_url,
                smtp.host = %config.smtp_relay_url,
                "SMTP relay does not match the IMAP provider; this may be a stale configuration created by the previous Yahoo fallback"
            );
        }

        Ok(config)
    }

    /// Saves configuration atomically with owner-only permissions because it may
    /// contain renewable OAuth credentials.
    pub async fn save(
        &self,
        path: &PathBuf,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let _save_guard = CONFIG_SAVE_LOCK.lock().await;
        self.save_unlocked(path).await
    }

    /// Applies a mutation to the latest configuration while holding the same lock
    /// through the complete read-modify-write transaction.
    pub async fn update<F>(
        path: &PathBuf,
        update: F,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>>
    where
        F: FnOnce(&mut Self),
    {
        let _save_guard = CONFIG_SAVE_LOCK.lock().await;
        let mut config = Self::init(path).await?;
        update(&mut config);
        config.save_unlocked(path).await?;
        Ok(config)
    }

    async fn save_unlocked(
        &self,
        path: &PathBuf,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let serialized = serde_json::to_string_pretty(self)?;
        let temporary_path = path.with_extension("json.tmp");
        let mut temporary_file = fs::File::create(&temporary_path).await?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&temporary_path, std::fs::Permissions::from_mode(0o600)).await?;
        }

        temporary_file.write_all(serialized.as_bytes()).await?;
        temporary_file.sync_all().await?;
        drop(temporary_file);
        fs::rename(&temporary_path, path).await?;

        Ok(())
    }
}

pub enum AppState {
    Initialized(Arc<InitializedState>),
    Fresh,
}

/// Coordinates startup status requests so the Keychain is read at most once
/// while the app is determining whether a returning user is signed in.
#[derive(Default)]
pub struct StartupStatusCache {
    pub status: tokio::sync::Mutex<Option<InitStatus>>,
    pub warning: tokio::sync::Mutex<Option<String>>,
}

impl AppState {
    pub fn state(&self) -> &InitializedState {
        match self {
            AppState::Initialized(state) => state,
            AppState::Fresh => panic!("Application not initialized"),
        }
    }
}

pub struct InitializedState {
    pub local_inference: LocalInference,
    pub sqlite_pool: SqlitePool,
    pub apple_keychain_manager: AppleKeychainManager,
    pub smtp_relay_url: String,
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
    tracing::info!(
        smtp.host = smtp_server_url,
        smtp.port = 465,
        "Initializing SMTP transport from registered relay"
    );

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

#[cfg(test)]
mod tests {
    use super::Config;

    const CONFIG_WITHOUT_SIGNATURE: &str = r#"{
        "imap_server_url": "imap.example.com",
        "smtp_relay_url": "smtp.example.com",
        "imap_port": 993,
        "sqlite_db": "emails.db",
        "accounts": [],
        "default_model": "test-model"
    }"#;

    #[test]
    fn missing_signature_deserializes_as_none() {
        let config: Config = serde_json::from_str(CONFIG_WITHOUT_SIGNATURE).unwrap();

        assert!(config.signature.is_none());
        assert!(config.signature_html.is_none());
    }

    #[test]
    fn none_signature_is_not_serialized() {
        let config: Config = serde_json::from_str(CONFIG_WITHOUT_SIGNATURE).unwrap();
        let serialized = serde_json::to_value(config).unwrap();

        assert!(serialized.get("signature").is_none());
        assert!(serialized.get("signature_html").is_none());
    }

    #[test]
    fn rich_signature_round_trips_without_changes() {
        let mut config: Config = serde_json::from_str(CONFIG_WITHOUT_SIGNATURE).unwrap();
        let signature_html =
            "<strong>Best regards,</strong><br>Aminu<br><a href=\"https://example.com\">Website</a>"
                .to_string();
        config.signature_html = Some(signature_html.clone());

        let serialized = serde_json::to_string(&config).unwrap();
        let deserialized: Config = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.signature_html, Some(signature_html));
    }
}
