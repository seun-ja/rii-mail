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
fn greet(
    state: tauri::State<AppState>,
    subject: String,
    email_from: String,
    body: String,
) -> Result<SpamRating, Error> {
    let email = SpamRaterRequest {
        from: email_from,
        subject,
        body,
    };

    // TODO: this blocking can be buggy - if the RPC call takes a long time, it will block the entire Tauri app. We should consider making this async and using a channel to send the result back to the main thread.
    let rating = tauri::async_runtime::block_on(rpc::caller(&state.rpc_client, email))?;

    Ok(rating)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub async fn run() {
    let state = config::AppState {
        rpc_client: init_rpc("0.0.0.0:5500").await.unwrap(),
    };

    tauri::Builder::default()
        .manage(state)
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
