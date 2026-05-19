use crate::rpc::AgentWorkerClient;
use async_native_tls::TlsStream;
use serde::{Deserialize, Serialize};
use tarpc::{client, serde_transport::tcp, tokio_serde::formats::Json};
use tauri::Manager as _;
use tokio::{fs, net::TcpStream, sync::RwLock};

#[derive(Deserialize, Serialize)]
pub struct Config {
    pub rpc_server: String,
    pub imap_server: String,
    pub imap_port: u16,
    #[serde(default)]
    pub rust_log: Option<String>,
    pub otlp_collector_endpoint: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            imap_server: "imap.gmail.com".to_string(),
            imap_port: 993,
            rpc_server: "0.0.0.0:5500".to_string(),
            rust_log: Some("info".to_string()),
            otlp_collector_endpoint: Some("http://otel-collector:4317".to_string()),
        }
    }
}

impl Config {
    pub async fn init(
        app: tauri::AppHandle,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let path = app.path().app_config_dir()?.join("config.json");

        let config_json = fs::read_to_string(path).await?;
        let config: Self = serde_json::from_str(&config_json)?;

        if config.imap_port == 0 || config.imap_server.is_empty() {
            panic!("IMAP Port and Server not set")
        }

        Ok(config)
    }
}

pub struct AppState {
    pub inner: RwLock<Option<InitializedState>>,
}

pub struct InitializedState {
    pub rpc_client: AgentWorkerClient,
    pub _imap_client: async_imap::Client<TlsStream<TcpStream>>,
}

pub async fn init_rpc(
    rpc_server: &str,
) -> Result<AgentWorkerClient, Box<dyn std::error::Error + Send + Sync>> {
    let transport = tcp::connect(rpc_server, Json::default).await?;

    let client = AgentWorkerClient::new(client::Config::default(), transport).spawn();

    tracing::info!("Connected to RPC Agent service 🤖");

    Ok(client)
}
