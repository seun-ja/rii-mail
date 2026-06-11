use arc_swap::ArcSwap;
use lettre::{message::header::ContentType, Message, Transport};
use sqlx::SqlitePool;
use tauri::Manager as _;
use tokio::sync::{mpsc::UnboundedSender, oneshot};

use crate::{
    config::AppState,
    db::{get_emails, MailBox, Provider},
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
pub async fn fetch_emails(
    app: tauri::AppHandle,
    min_range: u32,
    max_range: u32,
    mailbox: String,
    provider: String,
) -> Result<FetchEmailsResponse, Error> {
    let state = app.state::<ArcSwap<AppState>>();
    let current_state = state.load();
    let initialized = current_state.state();

    let provider = Provider::from(provider);
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

    let provider = Provider::from(provider);
    let mailbox = MailBox::from(mailbox);

    imap_cmd_channel_tx.send(ImapCommand::RefreshEmails {
        mail_box: mailbox.clone(),
        provider: provider.clone(),
        response_channel: fetch_update_tx,
    })?;

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
    provider: Provider,
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

#[tauri::command]
#[tracing::instrument(name = "command.email.send", skip(app))]
pub async fn send_email(
    app: tauri::AppHandle,
    to: (Option<String>, String),
    subject: String,
    body: String,
    content_type: String,
) -> Result<(), Error> {
    let state = app.state::<ArcSwap<AppState>>();
    let app_state = state.load();

    // TODO: make this dynamic
    let from: crate::handlers::MailAddress = app_state.state().accounts[0].clone().try_into()?;
    let to: crate::handlers::MailAddress = to.try_into()?;

    let message = Message::builder()
        .from(from.clone().into())
        .reply_to(from.into())
        .to(to.into())
        .subject(&subject)
        .header(content_type.parse().unwrap_or(ContentType::TEXT_PLAIN))
        .body(body)?;

    let response = app_state.state().smtp_transport_client.send(&message)?;

    tracing::info!("SMTP response: {:?}", response);

    Ok(())
}
