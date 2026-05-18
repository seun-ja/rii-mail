use crate::{
    config::{init_rpc, AppState},
    error::Error,
    rpc::{SpamRaterRequest, SpamRating},
};

mod config;
mod error;
mod rpc;

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
    let state = config::AppState {
        rpc_client: init_rpc("0.0.0.0:5500").await.unwrap(),
    };

    tauri::Builder::default()
        .manage(state)
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![rater])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
