use std::path::{Path, PathBuf};

use arc_swap::ArcSwap;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use chrono::Utc;
use lettre::{message::header::ContentType, Message, Transport};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use tauri::Manager as _;
use tokio::{
    fs::{self, OpenOptions},
    io::AsyncWriteExt as _,
    sync::{mpsc::UnboundedSender, oneshot},
};
use uuid::Uuid;

use crate::{
    config::AppState,
    db::{
        get_email_by_uid, get_emails, get_mailbox_uid_validity, populate_storage,
        reconcile_mailbox_uid_validity, set_mailbox_uid_validity, MailBox, Provider,
    },
    email_cache::{canonical_part_path, CompleteEmail, FrontendEmail, StandardEmail},
    error::Error,
    handlers::{populate_bcc_address, populate_cc_address, populate_to_address},
    imap::{FetchedAttachment, ImapCommand, RefreshSummary},
    workers::SharedFetchManager,
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

#[derive(Debug)]
pub struct EmailAttachmentData {
    pub file_name: String,
    pub data: Vec<u8>,
}

#[derive(Debug)]
struct AttachmentMetadata {
    file_name: String,
    content_type: String,
    part_id: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentPreviewResponse {
    pub file_name: String,
    pub content_type: String,
    pub base64_data: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadedAttachmentResponse {
    pub file_name: String,
    pub path: String,
}

#[tauri::command]
#[tracing::instrument(name = "command.email.fetch", skip(app), err)]
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

    let mailbox = MailBox::from(mailbox);

    let emails = get_emails_as_front_end_from_pool(
        initialized.accounts[0].provider.clone(),
        mailbox,
        min_range as u16,
        max_range as u16,
        &initialized.sqlite_pool,
    )
    .await?;

    Ok(FetchEmailsResponse { emails })
}

#[tauri::command]
#[tracing::instrument(name = "command.email.refresh", skip(app), err)]
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
        username: initialized.accounts[0].email.clone(),
        keychain_manager: initialized.apple_keychain_manager.clone(),
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

async fn get_email_attachment_metadata_from_pool(
    provider: Provider,
    mailbox: MailBox,
    uid: u32,
    part_id: &str,
    sqlite_pool: &SqlitePool,
) -> Result<AttachmentMetadata, Error> {
    let table_name = format!("{}_{}", provider.as_ref(), mailbox.as_ref());
    let email = get_email_by_uid(sqlite_pool, &table_name, uid)
        .await?
        .ok_or(Error::NotFound)?;
    let attachment = email
        .body
        .and_then(|content| {
            content
                .attachments
                .into_iter()
                .find(|attachment| attachment.part_id == part_id)
        })
        .ok_or(Error::NotFound)?;

    Ok(AttachmentMetadata {
        file_name: attachment.file_name,
        content_type: attachment.content_type,
        part_id: attachment.part_id,
    })
}

fn attachment_account_key(username: &str) -> String {
    hex::encode(Sha256::digest(
        username.trim().to_ascii_lowercase().as_bytes(),
    ))
}

pub(crate) fn attachment_cache_path(
    cache_root: &Path,
    username: &str,
    provider: &Provider,
    mailbox: &MailBox,
    uid_validity: u32,
    uid: u32,
    part_id: &str,
) -> Result<PathBuf, Error> {
    let path = canonical_part_path(part_id)
        .ok_or_else(|| Error::Other("Invalid attachment part identifier".to_string()))?;
    let part_key = path
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join("_");
    Ok(cache_root
        .join("attachments")
        .join(attachment_account_key(username))
        .join(provider.as_ref())
        .join(mailbox.as_ref())
        .join(uid_validity.to_string())
        .join(uid.to_string())
        .join(part_key))
}

fn attachment_file_name_candidate(file_name: &str, copy_number: u32) -> String {
    let safe_file_name = Path::new(file_name)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty() && *name != "." && *name != "..")
        .unwrap_or("attachment");

    if copy_number == 0 {
        return safe_file_name.to_string();
    }

    let path = Path::new(safe_file_name);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("attachment");

    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if !extension.is_empty() => {
            format!("{stem} ({copy_number}).{extension}")
        }
        _ => format!("{stem} ({copy_number})"),
    }
}

pub(crate) async fn save_attachment_to_directory(
    directory: &Path,
    attachment: &EmailAttachmentData,
) -> Result<PathBuf, Error> {
    fs::create_dir_all(directory).await?;

    for copy_number in 0..u32::MAX {
        let destination = directory.join(attachment_file_name_candidate(
            &attachment.file_name,
            copy_number,
        ));
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .await
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        };

        file.write_all(&attachment.data).await?;
        file.sync_all().await?;
        return Ok(destination);
    }

    Err(Error::Other(
        "Could not choose an available attachment filename".to_string(),
    ))
}

#[tauri::command]
#[tracing::instrument(
    name = "command.email.attachment.preview",
    skip(app),
    fields(uid, mailbox, part_id),
    err
)]
pub async fn get_email_attachment(
    app: tauri::AppHandle,
    uid: u32,
    mailbox: String,
    part_id: String,
) -> Result<AttachmentPreviewResponse, Error> {
    let mailbox = MailBox::from(mailbox);
    let (provider, pool, username, keychain_manager) = {
        let state = app.state::<ArcSwap<AppState>>();
        let current_state = state.load();
        let initialized = current_state.state();
        (
            initialized.accounts[0].provider.clone(),
            initialized.sqlite_pool.clone(),
            initialized.accounts[0].email.clone(),
            initialized.apple_keychain_manager.clone(),
        )
    };
    let metadata = get_email_attachment_metadata_from_pool(
        provider.clone(),
        mailbox.clone(),
        uid,
        &part_id,
        &pool,
    )
    .await?;
    let table_name = format!("{}_{}", provider.as_ref(), mailbox.as_ref());
    let cache_root = app.path().app_cache_dir()?;
    let stored_uid_validity = get_mailbox_uid_validity(&pool, &table_name).await?;
    let cached = match stored_uid_validity {
        Some(uid_validity) => {
            let path = attachment_cache_path(
                &cache_root,
                &username,
                &provider,
                &mailbox,
                uid_validity,
                uid,
                &metadata.part_id,
            )?;
            match fs::read(path).await {
                Ok(data) => Some(data),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.into()),
            }
        }
        None => None,
    };

    let data = match cached {
        Some(data) => data,
        None => {
            let imap_cmd_channel_tx = app.state::<UnboundedSender<ImapCommand>>();
            let (response_tx, response_rx) =
                oneshot::channel::<Result<FetchedAttachment, String>>();
            let cancel_token = app
                .state::<SharedFetchManager>()
                .logout_token
                .lock()
                .await
                .child_token();
            imap_cmd_channel_tx.send(ImapCommand::FetchAttachment {
                mail_box: mailbox.clone(),
                uid,
                part_id: metadata.part_id.clone(),
                response_channel: response_tx,
                username: username.clone(),
                keychain_manager,
                cancel_token,
            })?;
            let fetched = response_rx.await?.map_err(|message| {
                Error::Other(format!("Failed to fetch attachment: {message}"))
            })?;
            if stored_uid_validity.is_some_and(|uid_validity| uid_validity != fetched.uid_validity)
            {
                reconcile_mailbox_uid_validity(&pool, &table_name, fetched.uid_validity).await?;
                return Err(Error::Other(
                    "The mailbox changed on the server. Refresh it before opening attachments."
                        .to_string(),
                ));
            }
            set_mailbox_uid_validity(&pool, &table_name, fetched.uid_validity).await?;
            let cache_path = attachment_cache_path(
                &cache_root,
                &username,
                &provider,
                &mailbox,
                fetched.uid_validity,
                uid,
                &metadata.part_id,
            )?;
            if let Some(parent) = cache_path.parent() {
                fs::create_dir_all(parent).await?;
            }
            fs::write(&cache_path, &fetched.data).await?;
            fetched.data
        }
    };

    Ok(AttachmentPreviewResponse {
        file_name: metadata.file_name,
        content_type: metadata.content_type,
        base64_data: BASE64_STANDARD.encode(data),
    })
}

#[tauri::command]
#[tracing::instrument(
    name = "command.email.attachment.save",
    skip(app),
    fields(uid, mailbox, part_id),
    err
)]
pub async fn save_email_attachment(
    app: tauri::AppHandle,
    uid: u32,
    mailbox: String,
    part_id: String,
) -> Result<DownloadedAttachmentResponse, Error> {
    let mailbox = MailBox::from(mailbox);
    let (provider, pool, username) = {
        let state = app.state::<ArcSwap<AppState>>();
        let current_state = state.load();
        let initialized = current_state.state();
        (
            initialized.accounts[0].provider.clone(),
            initialized.sqlite_pool.clone(),
            initialized.accounts[0].email.clone(),
        )
    };
    let metadata = get_email_attachment_metadata_from_pool(
        provider.clone(),
        mailbox.clone(),
        uid,
        &part_id,
        &pool,
    )
    .await?;
    let table_name = format!("{}_{}", provider.as_ref(), mailbox.as_ref());
    let uid_validity = get_mailbox_uid_validity(&pool, &table_name)
        .await?
        .ok_or_else(|| Error::Other("Preview the attachment before downloading it".to_string()))?;
    let cache_path = attachment_cache_path(
        &app.path().app_cache_dir()?,
        &username,
        &provider,
        &mailbox,
        uid_validity,
        uid,
        &metadata.part_id,
    )?;
    let data = fs::read(cache_path).await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Error::Other("Preview the attachment before downloading it".to_string())
        } else {
            error.into()
        }
    })?;
    let attachment = EmailAttachmentData {
        file_name: metadata.file_name,
        data,
    };
    let saved_path = save_attachment_to_directory(&app.path().download_dir()?, &attachment).await?;
    let saved_file_name = saved_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&attachment.file_name)
        .to_string();

    Ok(DownloadedAttachmentResponse {
        file_name: saved_file_name,
        path: saved_path.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
#[tracing::instrument(name = "command.email.set_read", skip(app), fields(uid, read), err)]
pub async fn set_email_read(
    app: tauri::AppHandle,
    uid: u32,
    mailbox: String,
    read: bool,
) -> Result<(), Error> {
    let state = app.state::<ArcSwap<AppState>>();
    let current_state = state.load();
    let initialized = current_state.state();

    let imap_cmd_channel_tx = app.state::<UnboundedSender<ImapCommand>>();
    let (response_tx, response_rx) = oneshot::channel::<Result<(), String>>();

    imap_cmd_channel_tx.send(ImapCommand::SetEmailRead {
        mail_box: MailBox::from(mailbox),
        provider: initialized.accounts[0].provider.clone(),
        uid,
        read,
        response_channel: response_tx,
        username: initialized.accounts[0].email.clone(),
        keychain_manager: initialized.apple_keychain_manager.clone(),
    })?;

    response_rx
        .await?
        .map_err(|message| Error::Other(format!("Failed to synchronize read state: {message}")))
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
            let id = email.uid.map_or_else(
                || format!("db-{}-{}", mailbox.as_ref(), min_range + idx as u16),
                |uid| format!("db-{}-uid-{uid}", mailbox.as_ref()),
            );
            let mut frontend = email.into_frontend(id);
            frontend.folder = frontend_folder.clone();
            frontend
        })
        .collect();

    Ok(frontend)
}

#[tauri::command]
#[tracing::instrument(
    name = "command.email.send",
    skip_all,
    fields(to_count = to.len(), cc_count = cc.len(), bcc_count = bcc.len()),
    err
)]
pub async fn send_email(
    app: tauri::AppHandle,
    to: Vec<(Option<String>, String)>,
    cc: Vec<(Option<String>, String)>,
    bcc: Vec<(Option<String>, String)>,
    subject: String,
    body: String,
    content_type: String,
) -> Result<FrontendEmail, Error> {
    let state = app.state::<ArcSwap<AppState>>();
    let app_state = state.load();

    // TODO: make this dynamic
    let from: crate::handlers::MailAddress = app_state.state().accounts[0].clone().try_into()?;

    let builder = Message::builder()
        .from(from.clone().into())
        .reply_to(from.into());

    let to_populated_builder = populate_to_address(builder, to)?;
    let cc_populated_builder = populate_cc_address(to_populated_builder, cc)?;
    let bcc_populated_builder = populate_bcc_address(cc_populated_builder, bcc)?;

    let message = bcc_populated_builder
        .subject(&subject)
        .header(content_type.parse().unwrap_or(ContentType::TEXT_PLAIN))
        .body(body)?;

    tracing::info!(
        smtp.host = %app_state.state().smtp_relay_url,
        "Sending email through registered SMTP relay"
    );
    let response = app_state.state().smtp_transport_client.send(&message)?;
    let sent_at = Utc::now().fixed_offset();
    let local_uid = u32::from_be_bytes(
        Uuid::new_v4().as_bytes()[..4]
            .try_into()
            .expect("a UUID always contains four leading bytes"),
    );
    let sent_email = StandardEmail {
        uid: Some(local_uid),
        date: Some(sent_at),
        body: Some(message.formatted()),
        labels: Some(vec!["Sent".to_string()]),
        read: true,
    };
    let frontend_email = CompleteEmail::from(sent_email.clone())
        .into_frontend(format!("db-{}-uid-{local_uid}", MailBox::Sent.as_ref()));
    let sqlite_pool = app_state.state().sqlite_pool.clone();
    let sent_table_name = format!(
        "{}_{}",
        app_state.state().accounts[0].provider.as_ref(),
        MailBox::Sent.as_ref()
    );

    tauri::async_runtime::spawn(async move {
        if let Err(error) = populate_storage(&sqlite_pool, vec![sent_email], &sent_table_name).await
        {
            tracing::error!(?error, "Failed to persist sent email locally");
        }
    });

    tracing::info!("SMTP response: {:?}", response);

    Ok(frontend_email)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lettre::message::MessageBuilder;

    #[test]
    fn test_populate_to_address_success() {
        let builder = MessageBuilder::default();
        let addresses = vec![
            (
                Some("Recipient 1".to_string()),
                "recipient1@example.com".to_string(),
            ),
            (
                Some("Recipient 2".to_string()),
                "recipient2@example.com".to_string(),
            ),
        ];

        let result = populate_to_address(builder, addresses);
        assert!(result.is_ok());
    }

    #[test]
    fn test_populate_cc_address_success() {
        let builder = MessageBuilder::default();
        let addresses = vec![(Some("CC 1".to_string()), "cc1@example.com".to_string())];

        let result = populate_cc_address(builder, addresses);
        assert!(result.is_ok());
    }

    #[test]
    fn test_populate_bcc_address_success() {
        let builder = MessageBuilder::default();
        let addresses = vec![(Some("BCC 1".to_string()), "bcc1@example.com".to_string())];

        let result = populate_bcc_address(builder, addresses);
        assert!(result.is_ok());
    }

    #[test]
    fn test_populate_address_invalid_format() {
        let builder = MessageBuilder::default();
        let addresses = vec![(Some("Invalid".to_string()), "not-an-email".to_string())];

        let result = populate_to_address(builder, addresses);
        assert!(result.is_err());
    }

    #[test]
    fn test_populate_empty_addresses() {
        let builder = MessageBuilder::default();
        let addresses: Vec<(Option<String>, String)> = vec![];

        let result = populate_to_address(builder, addresses);
        assert!(result.is_ok());
    }
}
