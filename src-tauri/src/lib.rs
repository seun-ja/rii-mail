use tauri::Manager as _;
use tokio::sync::RwLock;

use crate::{
    config::{init_rpc, AppState, Config},
    error::Error,
    handlers::{config_setup, is_initialized, rater},
};

pub mod auth;
mod config;
pub mod email_cache;
mod error;
mod handlers;
mod rpc;
mod session;
mod tracing;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub async fn run() {
    let builder = tauri::Builder::default()
        .manage(AppState {
            inner: RwLock::new(None),
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            rater,
            config_setup,
            is_initialized
        ]);

    let app = builder
        .setup(|app| {
            let handle = app.handle().clone();

            tauri::async_runtime::spawn(async {
                if let Ok(config) = Config::init(handle.clone()).await {
                    initialize_services(handle, config).await.unwrap();
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!());

    app.expect("error while running tauri application");
}

async fn initialize_services(app: tauri::AppHandle, config: Config) -> Result<(), Error> {
    tracing::init_subscriber(
        &config.rust_log.unwrap_or_default(),
        &config.otlp_collector_endpoint.unwrap_or_default(),
    )
    .expect("Failed to initialize subscriber");

    let app_state = config::InitializedState {
        rpc_client: init_rpc(&config.rpc_server)
            .await
            .expect("Failed to Initialize RPC Service"),
        _imap_client: session::init_imap_session(&config.imap_server, config.imap_port)
            .await
            .expect("Failed to Initialize IMAP Session"),
    };

    let state = app.state::<AppState>();
    let mut inner = state.inner.write().await;

    *inner = Some(app_state);

    Ok(())
}
