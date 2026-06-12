use std::{path::PathBuf, sync::Arc};

use crate::{
    auth::AppleKeychainManager,
    db::Provider,
    error::{Error, ErrorMessage},
    rpc_llm::{AgentWorkerClient, Email},
};
use lettre::{transport::smtp::authentication::Credentials, SmtpTransport};
use llm::{builder::AgentBuilder, providers::Providers, tools::ToolWrapper, Agent};
use rig::tool::Tool;
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

#[derive(Deserialize, Serialize, Clone)]
pub struct Config {
    pub rpc_server_url: String,
    pub imap_server_url: String,
    pub smtp_relay_url: String,
    pub imap_port: u16,
    pub sqlite_db: String,
    pub accounts: Vec<String>,
    pub provider: String,
    pub default_model: String,
    pub api_key: String,
}

impl Config {
    pub async fn init(path: &PathBuf) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let config_json = fs::read_to_string(path).await?;
        let config: Self = serde_json::from_str(&config_json)?;

        if config.imap_port == 0 || config.imap_server_url.is_empty() {
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
    pub smtp_transport_client: SmtpTransport,
    pub llm_agent: Agent<Email>,
    pub accounts: Vec<Account>,
}

pub fn init_llm_agent<T: Tool>(
    provider: Providers,
    model: &str,
    api_key: &str,
    temperature: Option<f64>,
    max_tokens: Option<u64>,
    tools: Vec<ToolWrapper<T>>,
) -> Result<Agent<Email>, Error> {
    let agent = AgentBuilder::new(provider, "You're a helpful assistant", model) // TODO: work on the system message
        .api_key(api_key)
        .temperature(temperature.unwrap_or(0.0))
        .max_tokens(max_tokens.unwrap_or(3000))
        .tools(tools)
        .build_with_schema::<Email>()?;

    Ok(agent)
}

pub async fn init_rpc(rpc_server: &str) -> Result<AgentWorkerClient, Error> {
    let transport = tcp::connect(rpc_server, Json::default).await?;

    let client = AgentWorkerClient::new(client::Config::default(), transport).spawn();

    Ok(client)
}

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
