use arc_swap::ArcSwap;
use tauri::Manager as _;

use crate::{
    config::AppState,
    error::Error,
    llm::{Email, SpamRating},
};

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

    let message = serde_json::to_string(&email).map_err(|e| Error::Other(e.to_string()))?;

    let rating = initialized
        .local_inference
        .get()
        .await?
        .agent()
        .chat(&message, None)
        .await?;

    rating.try_into()
}
