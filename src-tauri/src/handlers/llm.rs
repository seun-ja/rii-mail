use arc_swap::ArcSwap;
use tauri::Manager as _;

use crate::{
    config::AppState,
    error::Error,
    rpc_llm::{caller, Email, SpamRating},
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

    let email = Email {
        from: email_from,
        subject,
        body,
    };

    let rating = caller(&initialized.rpc_llm_client.clone(), email).await?;

    Ok(rating)
}

#[tauri::command]
#[tracing::instrument(name = "command.email_generator", skip(app, _context, thoughts))]
pub async fn email_generator(
    app: tauri::AppHandle,
    thoughts: String,
    _context: Option<String>, // The idea here should be that it can take a file (pdf/docs) or past mails, or a link. This would be the base of assigning tools to the agent
) -> Result<Email, Error> {
    let state = app.state::<ArcSwap<AppState>>();
    let current_state = state.load();
    let initialized = current_state.state();

    // TODO: returning a stream instead
    let message = initialized.llm_agent.agent().schema_chat(&thoughts).await?;

    Ok(message)
}
