use crate::{
    config::InitializedState,
    error::Error,
    rpc::{self, SpamRaterRequest, SpamRating},
};

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
#[tracing::instrument(name = "command.rater", skip(state))]
pub async fn rater(
    state: tauri::State<'_, InitializedState>,
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
