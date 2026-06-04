use arc_swap::ArcSwap;
use tauri::Manager as _;

use crate::{
    config::AppState,
    error::Error,
    llm::{caller, EmailRequest, SpamRating},
};

#[tauri::command]
#[tracing::instrument(name = "command.rater", skip(app, subject, body, email_from))]
pub async fn rater(
    app: tauri::AppHandle,
    subject: String,
    email_from: String,
    body: String,
) -> Result<SpamRating, Error> {
    let state = app.state::<ArcSwap<AppState>>();
    let current_state = state.load();
    let initialized = current_state.state();

    let email = EmailRequest {
        from: email_from,
        subject,
        body,
    };

    let rating = caller(&initialized.rpc_llm_client.clone(), email).await?;

    Ok(rating)
}
