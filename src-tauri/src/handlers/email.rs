use arc_swap::ArcSwap;
use tauri::Manager as _;

use crate::{
    config::AppState,
    db::get_emails,
    email_cache::{CompleteEmail, FrontendEmail},
    error::Error,
};

#[tauri::command]
#[tracing::instrument(name = "command.email.fetch", skip(app))]
pub async fn fetch_emails_handler(
    app: tauri::AppHandle,
    min_range: u32,
    max_range: u32,
) -> Result<Vec<FrontendEmail>, Error> {
    let state = app.state::<ArcSwap<AppState>>();
    let current_state = state.load();
    let initialized = current_state.state();

    let emails: Vec<CompleteEmail> =
        get_emails(&initialized.sqlite_pool, min_range, max_range).await?;

    let frontend = emails
        .into_iter()
        .enumerate()
        .map(|(idx, email)| email.into_frontend(format!("db-{}", min_range + idx as u32)))
        .collect();

    Ok(frontend)
}
