use arc_swap::ArcSwap;
use tauri::Manager as _;

use crate::{
    config::AppState,
    db::{get_emails, MailBox, Providers},
    email_cache::{CompleteEmail, FrontendEmail},
    error::Error,
};

#[tauri::command]
#[tracing::instrument(name = "command.email.fetch", skip(app))]
pub async fn fetch_emails_handler(
    app: tauri::AppHandle,
    min_range: u32,
    max_range: u32,
    mailbox: String,
    provider: String,
) -> Result<Vec<FrontendEmail>, Error> {
    let state = app.state::<ArcSwap<AppState>>();
    let current_state = state.load();
    let initialized = current_state.state();

    let provider = Providers::from(provider);
    let mailbox = MailBox::from(mailbox);

    let table_name = format!("{}_{}", provider.as_ref(), mailbox.as_ref());

    let emails: Vec<CompleteEmail> =
        get_emails(&initialized.sqlite_pool, &table_name, min_range, max_range).await?;

    let frontend_folder = mailbox.as_ref().to_string();

    let frontend = emails
        .into_iter()
        .enumerate()
        .map(|(idx, email)| {
            let mut frontend = email.into_frontend(format!(
                "db-{}-{}",
                mailbox.as_ref(),
                min_range + idx as u32
            ));
            frontend.folder = frontend_folder.clone();
            frontend
        })
        .collect();

    Ok(frontend)
}
