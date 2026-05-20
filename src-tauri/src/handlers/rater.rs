use crate::{
    config::AppState,
    error::Error,
    rpc::{self, SpamRaterRequest, SpamRating},
};

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
#[tracing::instrument(name = "command.rater", skip(state))]
pub async fn rater(
    state: tauri::State<'_, std::sync::Arc<tokio::sync::RwLock<AppState>>>,
    subject: String,
    email_from: String,
    body: String,
) -> Result<SpamRating, Error> {
    let app_state = state.read().await;
    let initialized = app_state.state();

    let email = SpamRaterRequest {
        from: email_from,
        subject,
        body,
    };
    rpc::caller(&initialized.rpc_client, email).await
}
