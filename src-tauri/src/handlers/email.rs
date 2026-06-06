use arc_swap::ArcSwap;
use sqlx::SqlitePool;
use tauri::Manager as _;
use tokio::sync::{mpsc::UnboundedSender, oneshot};

use crate::{
    config::AppState,
    db::{get_emails, MailBox, Providers},
    email_cache::{CompleteEmail, FrontendEmail},
    error::Error,
    imap::{ImapCommand, RefreshSummary},
};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchEmailsResponse {
    pub emails: Vec<FrontendEmail>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshEmailsResponse {
    pub emails: Vec<FrontendEmail>,
    pub new_emails_count: u16,
    pub total_emails: u32,
}

#[tauri::command]
#[tracing::instrument(name = "command.email.fetch", skip(app))]
pub async fn fetch_emails_handler(
    app: tauri::AppHandle,
    min_range: u32,
    max_range: u32,
    mailbox: String,
    provider: String,
) -> Result<FetchEmailsResponse, Error> {
    let state = app.state::<ArcSwap<AppState>>();
    let current_state = state.load();
    let initialized = current_state.state();

    let provider = Providers::from(provider);
    let mailbox = MailBox::from(mailbox);

    let emails = get_emails_as_front_end_from_pool(
        provider,
        mailbox,
        min_range as u16,
        max_range as u16,
        &initialized.sqlite_pool,
    )
    .await?;

    Ok(FetchEmailsResponse { emails })
}

#[tauri::command]
#[tracing::instrument(name = "command.email.refresh", skip(app))]
pub async fn refresh_emails_handler(
    app: tauri::AppHandle,
    mailbox: String,
    provider: String,
) -> Result<RefreshEmailsResponse, Error> {
    let state = app.state::<ArcSwap<AppState>>();
    let current_state = state.load();
    let initialized = current_state.state();

    let imap_cmd_channel_tx = app.state::<UnboundedSender<ImapCommand>>();
    let (fetch_update_tx, fetch_update_rx) = oneshot::channel::<Result<RefreshSummary, String>>();

    let provider = Providers::from(provider);
    let mailbox = MailBox::from(mailbox);

    imap_cmd_channel_tx.send(ImapCommand::RefreshEmails(
        mailbox.clone(),
        provider.clone(),
        fetch_update_tx,
    ))?;

    match fetch_update_rx.await? {
        Ok(refresh_summary) => {
            tracing::info!(
                new_emails = refresh_summary.new_emails_count,
                total_emails = refresh_summary.total_emails,
                "Fetched latest mailbox state"
            );

            let emails = get_emails_as_front_end_from_pool(
                provider,
                mailbox,
                0,
                refresh_summary.new_emails_count,
                &initialized.sqlite_pool,
            )
            .await?;

            Ok(RefreshEmailsResponse {
                emails,
                new_emails_count: refresh_summary.new_emails_count,
                total_emails: refresh_summary.total_emails,
            })
        }
        Err(err) => {
            tracing::error!(error = %err, "Failed to fetch new emails");
            Err(Error::Other(format!("Failed to fetch new emails: {}", err)))
        }
    }
}

pub(crate) async fn get_emails_as_front_end_from_pool(
    provider: Providers,
    mailbox: MailBox,
    min_range: u16,
    max_range: u16,
    sqlite_pool: &SqlitePool,
) -> Result<Vec<FrontendEmail>, Error> {
    let table_name = format!("{}_{}", provider.as_ref(), mailbox.as_ref());

    let emails: Vec<CompleteEmail> =
        get_emails(sqlite_pool, &table_name, min_range, max_range).await?;

    let frontend_folder = mailbox.as_ref().to_string();

    let frontend = emails
        .into_iter()
        .enumerate()
        .map(|(idx, email)| {
            let mut frontend = email.into_frontend(format!(
                "db-{}-{}",
                mailbox.as_ref(),
                min_range + idx as u16
            ));
            frontend.folder = frontend_folder.clone();
            frontend
        })
        .collect();

    Ok(frontend)
}
