use crate::rpc::AgentWorkerClient;
use async_native_tls::TlsStream;
use serde::Deserialize;
use tarpc::{client, serde_transport::tcp, tokio_serde::formats::Json};
use tokio::net::TcpStream;

#[derive(Deserialize)]
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
    pub fn init() -> Self {
        let config = envy::from_env::<Self>()
            .map_err(|e| eprint!("Failed to load config: {}", e))
            .unwrap_or_default();

        config
    }
}

pub struct AppState {
    pub rpc_client: AgentWorkerClient,
    pub _imap_client: async_imap::Client<TlsStream<TcpStream>>,
}

pub async fn init_rpc(rpc_server: &str) -> Result<AgentWorkerClient, Box<dyn std::error::Error>> {
    let transport = tcp::connect(rpc_server, Json::default).await?;

    let client = AgentWorkerClient::new(client::Config::default(), transport).spawn();

    Ok(client)
}
