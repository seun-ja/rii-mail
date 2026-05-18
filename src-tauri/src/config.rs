use crate::rpc::AgentWorkerClient;
use tarpc::{client, serde_transport::tcp, tokio_serde::formats::Json};

#[derive(Clone)]
pub struct AppState {
    pub rpc_client: AgentWorkerClient,
}

pub async fn init_rpc(rpc_server: &str) -> Result<AgentWorkerClient, Box<dyn std::error::Error>> {
    let transport = tcp::connect(rpc_server, Json::default).await?;

    let client = AgentWorkerClient::new(client::Config::default(), transport).spawn();

    Ok(client)
}
