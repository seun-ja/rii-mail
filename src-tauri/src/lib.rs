use crate::{
    config::{init_rpc, AppState},
    error::Error,
    rpc::{SpamRaterRequest, SpamRating},
};

pub mod auth;
mod config;
pub mod email_cache;
mod error;
mod rpc;
mod session;
mod tracing;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
async fn rater(
    state: tauri::State<'_, AppState>,
    subject: String,
    email_from: String,
    body: String,
) -> Result<SpamRating, Error> {
    let email = SpamRaterRequest {
        from: email_from,
        subject,
        body,
    };

    rpc::caller(&state.rpc_client, email).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub async fn run() {
    let config = config::Config::init();

    tracing::init_subscriber(
        &config.rust_log.unwrap_or_default(),
        &config.otlp_collector_endpoint.unwrap_or_default(),
    )
    .expect("Failed to initialize subscriber");

    let state = config::AppState {
        rpc_client: init_rpc(&config.rpc_server).await.expect(""),
        _imap_client: session::init_imap_session(&config.imap_server, config.imap_port)
            .await
            .expect(""),
    };

    tauri::Builder::default()
        .manage(state)
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![rater])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
