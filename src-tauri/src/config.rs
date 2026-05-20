use std::path::PathBuf;

use crate::rpc::AgentWorkerClient;
use async_native_tls::TlsStream;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tarpc::{client, serde_transport::tcp, tokio_serde::formats::Json};
use tokio::{fs, net::TcpStream};

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    Initialized(InitializedState),
    Fresh,
}

impl AppState {
    pub fn state(&self) -> &InitializedState {
        match self {
            AppState::Initialized(state) => state,
            AppState::Fresh => panic!("Application not initialized"),
        }
    }

    pub fn state_mut(&mut self) -> &mut InitializedState {
        match self {
            AppState::Initialized(state) => state,
            AppState::Fresh => panic!("Application not initialized"),
        }
    }
}

pub struct InitializedState {
    pub rpc_client: AgentWorkerClient,
    pub sqlite_pool: SqlitePool,
    pub imap_session: async_imap::Session<TlsStream<TcpStream>>,
}

pub async fn init_rpc(
    rpc_server: &str,
) -> Result<AgentWorkerClient, Box<dyn std::error::Error + Send + Sync>> {
    let transport = tcp::connect(rpc_server, Json::default).await?;

    let client = AgentWorkerClient::new(client::Config::default(), transport).spawn();

    tracing::info!("Connected to RPC Agent service 🤖");

    Ok(client)
}
