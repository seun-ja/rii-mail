use std::path::{Path, PathBuf};

use arc_swap::ArcSwap;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use chrono::Utc;
use lettre::{message::header::ContentType, Message, Transport};
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
        cached_parts_for_eviction, cached_parts_size, get_attachment_record, get_cached_part,
        get_emails, get_mailbox_uid_validity, populate_storage, reconcile_mailbox_uid_validity,
        remove_cached_part, store_cached_part, AttachmentRecord, MailBox, Provider,
    },
    email_cache::{CompleteEmail, FrontendEmail, StandardEmail},
    error::Error,
    handlers::{populate_bcc_address, populate_cc_address, populate_to_address},
    imap::{FetchedAttachment, ImapCommand, RefreshSummary},
    workers::SharedFetchManager,
};

const ATTACHMENT_CACHE_LIMIT_BYTES: i64 = 256 * 1024 * 1024;

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

fn attachment_cache_path(cache_root: &Path, part_id: i64) -> (String, PathBuf) {
    let relative = format!("attachments/{part_id}");
    let absolute = cache_root.join(&relative);
    (relative, absolute)
}

async fn evict_attachment_cache(
    pool: &SqlitePool,
    cache_root: &Path,
    protected_part_id: i64,
) -> Result<(), Error> {
    let mut total = cached_parts_size(pool).await?;
    if total <= ATTACHMENT_CACHE_LIMIT_BYTES {
        return Ok(());
    }

    for cached in cached_parts_for_eviction(pool, protected_part_id).await? {
        if total <= ATTACHMENT_CACHE_LIMIT_BYTES {
            break;
        }
        let path = cache_root.join(&cached.relative_path);
        match fs::remove_file(&path).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::warn!(?error, path = %path.display(), "Unable to evict attachment file");
                continue;
            }
        }
        remove_cached_part(pool, cached.part_id).await?;
        total = total.saturating_sub(cached.byte_size);
    }

    Ok(())
}

async fn load_attachment_data(
    app: &tauri::AppHandle,
    uid: u32,
    mailbox: &MailBox,
    metadata: &AttachmentRecord,
) -> Result<Vec<u8>, Error> {
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
    let cache_root = app.path().app_cache_dir()?;

    if let Some(cached) = get_cached_part(&pool, metadata.part_db_id).await? {
        match fs::read(cache_root.join(&cached.relative_path)).await {
            Ok(data) => return Ok(data),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                remove_cached_part(&pool, metadata.part_db_id).await?;
            }
            Err(error) => return Err(error.into()),
        }
    }

    let imap_cmd_channel_tx = app.state::<UnboundedSender<ImapCommand>>();
    let (response_tx, response_rx) = oneshot::channel::<Result<FetchedAttachment, String>>();
    let cancel_token = app
        .state::<SharedFetchManager>()
        .logout_token
        .lock()
        .await
        .child_token();
    imap_cmd_channel_tx.send(ImapCommand::FetchAttachment {
        mail_box: mailbox.clone(),
        uid,
        part_id: metadata.section.clone(),
        response_channel: response_tx,
        username,
        keychain_manager,
        cancel_token,
    })?;
    let fetched = response_rx
        .await?
        .map_err(|message| Error::Other(format!("Failed to fetch attachment: {message}")))?;

    let mailbox_key = format!("{}_{}", provider.as_ref(), mailbox.as_ref());
    let previous_uid_validity = get_mailbox_uid_validity(&pool, &mailbox_key).await?;
    if previous_uid_validity.is_some_and(|value| value != fetched.uid_validity) {
        reconcile_mailbox_uid_validity(&pool, &mailbox_key, fetched.uid_validity).await?;
        return Err(Error::Other(
            "The mailbox changed on the server. Refresh it before opening attachments.".to_string(),
        ));
    }

    let (relative_path, cache_path) = attachment_cache_path(&cache_root, metadata.part_db_id);
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let temporary_path = cache_path.with_extension("tmp");
    fs::write(&temporary_path, &fetched.data).await?;
    fs::rename(&temporary_path, &cache_path).await?;
    store_cached_part(
        &pool,
        metadata.part_db_id,
        &relative_path,
        fetched.data.len(),
    )
    .await?;
    evict_attachment_cache(&pool, &cache_root, metadata.part_db_id).await?;

    Ok(fetched.data)
}

fn attachment_file_name_candidate(file_name: &str, copy_number: u32) -> String {
    let safe_name = Path::new(file_name)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty() && *name != "." && *name != "..")
        .unwrap_or("attachment");
    if copy_number == 0 {
        return safe_name.to_string();
    }
    let path = Path::new(safe_name);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("attachment");
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if !extension.is_empty() => format!("{stem} ({copy_number}).{extension}"),
        _ => format!("{stem} ({copy_number})"),
    }
}

async fn save_attachment_to_directory(
    directory: &Path,
    file_name: &str,
    data: &[u8],
) -> Result<PathBuf, Error> {
    fs::create_dir_all(directory).await?;
    for copy_number in 0..u32::MAX {
        let destination = directory.join(attachment_file_name_candidate(file_name, copy_number));
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
        file.write_all(data).await?;
        file.sync_all().await?;
        return Ok(destination);
    }
    Err(Error::Other(
        "Could not choose an available attachment filename".to_string(),
    ))
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
        min_range,
        max_range,
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
                u32::from(refresh_summary.new_emails_count),
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
    let (provider, pool) = {
        let state = app.state::<ArcSwap<AppState>>();
        let current_state = state.load();
        let initialized = current_state.state();
        (
            initialized.accounts[0].provider.clone(),
            initialized.sqlite_pool.clone(),
        )
    };
    let mailbox_key = format!("{}_{}", provider.as_ref(), mailbox.as_ref());
    let metadata = get_attachment_record(&pool, &mailbox_key, uid, &part_id)
        .await?
        .ok_or(Error::NotFound)?;
    let data = load_attachment_data(&app, uid, &mailbox, &metadata).await?;

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
    let (provider, pool) = {
        let state = app.state::<ArcSwap<AppState>>();
        let current_state = state.load();
        let initialized = current_state.state();
        (
            initialized.accounts[0].provider.clone(),
            initialized.sqlite_pool.clone(),
        )
    };
    let mailbox_key = format!("{}_{}", provider.as_ref(), mailbox.as_ref());
    let metadata = get_attachment_record(&pool, &mailbox_key, uid, &part_id)
        .await?
        .ok_or(Error::NotFound)?;
    let data = load_attachment_data(&app, uid, &mailbox, &metadata).await?;
    let saved_path =
        save_attachment_to_directory(&app.path().download_dir()?, &metadata.file_name, &data)
            .await?;
    let saved_file_name = saved_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&metadata.file_name)
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
    min_range: u32,
    max_range: u32,
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
                || format!("db-{}-{}", mailbox.as_ref(), min_range + idx as u32),
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
