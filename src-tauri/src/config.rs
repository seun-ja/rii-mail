use std::{path::PathBuf, sync::Arc};

use crate::llm::LlmProvider;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::fs;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum InitStatus {
    /// Configuration file doesn't exist - user needs to complete full setup
    Setup,
    /// Configuration exists but user is not authenticated - show login page
    Login,
    /// User is fully authenticated and app is ready
    SignedIn,
}

#[derive(Deserialize, Serialize)]
pub struct Config {
    pub rpc_server: String,
    pub imap_server: String,
    pub imap_port: u16,
    pub sqlite_db: String,
    #[serde(default)]
    pub rust_log: Option<String>,
    pub otlp_collector_endpoint: Option<String>,
    pub email_cache_size: u32,
}

impl Config {
    pub async fn init(path: &PathBuf) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let config_json = fs::read_to_string(path).await?;
        let config: Self = serde_json::from_str(&config_json)?;

        if config.imap_port == 0 || config.imap_server.is_empty() {
            return Err("IMAP Port and Server not set".into());
        }

        Ok(config)
    }
}

pub enum AppState {
    Initialized(Arc<InitializedState>),
    Fresh,
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
    pub llm_client: LlmProvider,
    pub sqlite_pool: SqlitePool,
}

pub struct ImapClientConfig {
    pub username: String,
    pub password: String,
    pub imap_server: String,
    pub imap_port: u16,
    pub sqlite_pool: SqlitePool,
}
