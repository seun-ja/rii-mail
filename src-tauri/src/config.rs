use std::{path::PathBuf, sync::Arc};

use crate::{auth::AppleKeychainManager, error::Error, llm::AgentWorkerClient};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tarpc::{client, serde_transport::tcp, tokio_serde::formats::Json};
use tokio::fs;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum InitStatus {
    /// Configuration file doesn't exist - user needs to complete full setup
    Setup,
    /// Configuration exists but user is not authenticated - show login page
    Login,
    /// User is fully authenticated and app is ready
    SignedIn,
    /// User is returning and already authenticated
    ReturningSigned,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Config {
    pub rpc_server: String,
    pub imap_server: String,
    pub imap_port: u16,
    pub sqlite_db: String,
    pub accounts: Vec<String>,
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
    // pub _llm_client: LlmProvider,
    pub rpc_llm_client: AgentWorkerClient,
    pub sqlite_pool: SqlitePool,
    pub apple_keychain_manager: AppleKeychainManager,
}

pub async fn init_rpc(rpc_server: &str) -> Result<AgentWorkerClient, Error> {
    let transport = tcp::connect(rpc_server, Json::default).await?;

    let client = AgentWorkerClient::new(client::Config::default(), transport).spawn();

    Ok(client)
}

pub struct ImapClientConfig {
    pub username: String,
    pub password: String,
    pub imap_server: String,
    pub imap_port: u16,
    pub sqlite_pool: SqlitePool,
    pub login_result_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
}

pub struct ReturningUserImapClientConfig {
    pub username: String,
    pub imap_server: String,
    pub imap_port: u16,
    pub apple_keychain_manager: AppleKeychainManager,
    pub sqlite_pool: SqlitePool,
    pub login_result_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
}
