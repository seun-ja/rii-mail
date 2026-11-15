use std::{collections::HashSet, sync::Arc, time::Duration};

use async_imap::{
    imap_proto::types::{
        BodyContentCommon, BodyContentSinglePart, BodyParams, BodyStructure, MessageSection,
        SectionPath,
    },
    types::{Fetch, Flag, NameAttribute},
    Session,
};
use async_native_tls::TlsStream;
use futures::StreamExt;
use sqlx::SqlitePool;
use tauri::Emitter as _;
use tokio::{
    net::TcpStream,
    sync::{oneshot::Sender, Mutex},
    time::timeout,
};
use tokio_util::sync::CancellationToken;
use tracing::Instrument as _;

use crate::{
    db::{
        get_last_uid, populate_inbox_folder_count, populate_sent_folder_count, populate_storage,
        reconcile_mailbox_uid_validity, replace_storage, reset_storage, set_email_read_states,
        set_last_uid, MailBox, Provider,
    },
    email_cache::{
        encode_cached_email_content, normalize_attachment_file_name, CompleteEmail,
        EmailAttachment, EmailContent, SharedImapSession, StandardEmail,
    },
    error::Error,
    imap::FetchedAttachment,
    llm::{phishing_rater, Label},
    INBOX_POPULATE_UPDATE, SENT_POPULATE_UPDATE, SPAM_POPULATE_UPDATE,
};

pub enum FetchResult {
    EmptyMailbox,
    Populated,
    Fetched { count: u16, total_emails: u32 },
}

impl FetchResult {
    pub fn count(&self) -> u16 {
        match self {
            FetchResult::EmptyMailbox => 0,
            FetchResult::Populated => 0,
            FetchResult::Fetched { count, .. } => *count,
        }
    }

    pub fn total_emails(&self) -> u32 {
        match self {
            FetchResult::EmptyMailbox => 0,
            FetchResult::Populated => 0,
            FetchResult::Fetched { total_emails, .. } => *total_emails,
        }
    }
}

const INITIAL_SYNC_CHUNK_SIZE: usize = 20;
const IMAP_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const FETCH_MESSAGE_ATTRIBUTES: &str = "(UID FLAGS INTERNALDATE BODYSTRUCTURE BODY.PEEK[HEADER])";

#[derive(Debug, Clone)]
pub(crate) struct BodyPartPlan {
    pub part_id: String,
    pub path: Vec<u32>,
    pub html: bool,
}

#[derive(Debug)]
struct MessageFetchPlan {
    uid: u32,
    date: Option<chrono::DateTime<chrono::FixedOffset>>,
    header: Vec<u8>,
    labels: Option<Vec<String>>,
    read: bool,
    body_parts: Vec<BodyPartPlan>,
    attachments: Vec<EmailAttachment>,
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                decoded.push(byte);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }

    String::from_utf8_lossy(&decoded).into_owned()
}

fn decode_extended_param(value: &str) -> String {
    let encoded = value
        .split_once('\'')
        .and_then(|(_, remainder)| remainder.split_once('\''))
        .map_or(value, |(_, encoded)| encoded);
    percent_decode(encoded)
}

fn body_param(params: &BodyParams<'_>, name: &str) -> Option<String> {
    let params = params.as_ref()?;

    if let Some(value) = params
        .iter()
        .find_map(|(key, value)| key.eq_ignore_ascii_case(name).then_some(value.as_ref()))
    {
        return Some(value.to_string());
    }

    let extended_name = format!("{name}*");
    if let Some(value) = params.iter().find_map(|(key, value)| {
        key.eq_ignore_ascii_case(&extended_name)
            .then_some(value.as_ref())
    }) {
        return Some(decode_extended_param(value));
    }

    let lower_prefix = format!("{}*", name.to_ascii_lowercase());
    let mut continuations = params
        .iter()
        .filter_map(|(key, value)| {
            let lower_key = key.to_ascii_lowercase();
            let suffix = lower_key.strip_prefix(&lower_prefix)?;
            let index = suffix.trim_end_matches('*').parse::<usize>().ok()?;
            Some((index, suffix.ends_with('*'), value.as_ref()))
        })
        .collect::<Vec<_>>();
    continuations.sort_unstable_by_key(|(index, _, _)| *index);

    if continuations.is_empty()
        || continuations
            .iter()
            .enumerate()
            .any(|(expected, (actual, _, _))| expected != *actual)
    {
        return None;
    }

    Some(
        continuations
            .into_iter()
            .map(|(index, encoded, value)| {
                if encoded {
                    if index == 0 {
                        decode_extended_param(value)
                    } else {
                        percent_decode(value)
                    }
                } else {
                    value.to_string()
                }
            })
            .collect(),
    )
}

fn render_part_id(path: &[u32]) -> String {
    path.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

fn collect_single_part(
    common: &BodyContentCommon<'_>,
    other: &BodyContentSinglePart<'_>,
    path: Vec<u32>,
    is_text: bool,
    force_attachment: bool,
    within_related: bool,
    body_parts: &mut Vec<BodyPartPlan>,
    attachments: &mut Vec<EmailAttachment>,
) {
    let attachment_id = attachments.len() as u32;
    let file_name = common
        .disposition
        .as_ref()
        .and_then(|disposition| body_param(&disposition.params, "filename"))
        .or_else(|| body_param(&common.ty.params, "name"));
    let disposition_is_attachment = common
        .disposition
        .as_ref()
        .is_some_and(|disposition| disposition.ty.eq_ignore_ascii_case("attachment"));
    let disposition_is_inline = common
        .disposition
        .as_ref()
        .is_some_and(|disposition| disposition.ty.eq_ignore_ascii_case("inline"));
    let is_image = common.ty.ty.eq_ignore_ascii_case("image");
    let embedded_inline = !disposition_is_attachment
        && (disposition_is_inline || within_related)
        && (other.id.is_some() || is_image);
    let is_attachment =
        force_attachment || disposition_is_attachment || (file_name.is_some() && !embedded_inline);
    let server_part_id = render_part_id(&path);

    if is_attachment {
        let content_type = format!("{}/{}", common.ty.ty, common.ty.subtype).to_ascii_lowercase();
        attachments.push(EmailAttachment {
            id: attachment_id,
            part_id: server_part_id,
            file_name: normalize_attachment_file_name(file_name.as_deref(), attachment_id),
            content_type,
            size: other.octets as usize,
            data: Vec::new(),
        });
    } else if is_text && common.ty.ty.eq_ignore_ascii_case("text") {
        body_parts.push(BodyPartPlan {
            part_id: server_part_id,
            path,
            html: common.ty.subtype.eq_ignore_ascii_case("html"),
        });
    }
}

pub(crate) fn collect_body_structure_parts(
    structure: &BodyStructure<'_>,
) -> (Vec<BodyPartPlan>, Vec<EmailAttachment>) {
    fn walk(
        structure: &BodyStructure<'_>,
        path: Vec<u32>,
        within_related: bool,
        body_parts: &mut Vec<BodyPartPlan>,
        attachments: &mut Vec<EmailAttachment>,
    ) {
        match structure {
            BodyStructure::Multipart { common, bodies, .. } => {
                let within_related =
                    within_related || common.ty.subtype.eq_ignore_ascii_case("related");
                for (index, body) in bodies.iter().enumerate() {
                    let mut child_path = path.clone();
                    child_path.push(index as u32 + 1);
                    walk(body, child_path, within_related, body_parts, attachments);
                }
            }
            BodyStructure::Text { common, other, .. } => collect_single_part(
                common,
                other,
                path,
                true,
                false,
                within_related,
                body_parts,
                attachments,
            ),
            BodyStructure::Basic { common, other, .. } => collect_single_part(
                common,
                other,
                path,
                false,
                false,
                within_related,
                body_parts,
                attachments,
            ),
            BodyStructure::Message { common, other, .. } => collect_single_part(
                common,
                other,
                path,
                false,
                true,
                within_related,
                body_parts,
                attachments,
            ),
        }
    }

    let mut body_parts = Vec::new();
    let mut attachments = Vec::new();
    let initial_path = if matches!(structure, BodyStructure::Multipart { .. }) {
        Vec::new()
    } else {
        vec![1]
    };
    walk(
        structure,
        initial_path,
        false,
        &mut body_parts,
        &mut attachments,
    );
    (body_parts, attachments)
}

fn message_fetch_plan(fetch: &Fetch) -> Result<MessageFetchPlan, Error> {
    let uid = fetch
        .uid
        .ok_or_else(|| Error::Other("IMAP metadata response did not include a UID".to_string()))?;
    let header = fetch.header().ok_or_else(|| {
        Error::Other(format!(
            "IMAP metadata for UID {uid} did not include headers"
        ))
    })?;
    let structure = fetch.bodystructure().ok_or_else(|| {
        Error::Other(format!(
            "IMAP metadata for UID {uid} did not include BODYSTRUCTURE"
        ))
    })?;
    let (body_parts, attachments) = collect_body_structure_parts(structure);

    Ok(MessageFetchPlan {
        uid,
        date: fetch.internal_date(),
        header: header.to_vec(),
        labels: fetch
            .gmail_labels()
            .map(|labels| labels.iter().map(ToString::to_string).collect()),
        read: fetch.flags().any(|flag| flag == Flag::Seen),
        body_parts,
        attachments,
    })
}

fn mime_part_text(fetch: &Fetch, part: &BodyPartPlan) -> Option<String> {
    let mime_path = SectionPath::Part(part.path.clone(), Some(MessageSection::Mime));
    let body_path = SectionPath::Part(part.path.clone(), None);
    let mime = fetch.section(&mime_path)?;
    let body = fetch.section(&body_path)?;
    let mut raw = mime.to_vec();
    if !raw.ends_with(b"\r\n\r\n") {
        if !raw.ends_with(b"\r\n") {
            raw.extend_from_slice(b"\r\n");
        }
        raw.extend_from_slice(b"\r\n");
    }
    raw.extend_from_slice(body);

    let parsed = mail_parser::MessageParser::default().parse(&raw);
    let decoded = if part.html {
        parsed.and_then(|message| message.body_html(0).map(|body| body.into_owned()))
    } else {
        parsed.and_then(|message| message.body_text(0).map(|body| body.into_owned()))
    };

    decoded.or_else(|| Some(String::from_utf8_lossy(body).into_owned()))
}

async fn fetch_message_without_attachments(
    session: &mut Session<TlsStream<TcpStream>>,
    plan: MessageFetchPlan,
    cancel_token: &CancellationToken,
) -> Result<StandardEmail, Error> {
    let mut text_body = Vec::new();
    let mut html_body = Vec::new();

    if !plan.body_parts.is_empty() {
        let sections = plan
            .body_parts
            .iter()
            .flat_map(|part| {
                [
                    format!("BODY.PEEK[{}.MIME]", part.part_id),
                    format!("BODY.PEEK[{}]", part.part_id),
                ]
            })
            .collect::<Vec<_>>()
            .join(" ");
        let query = format!("(UID {sections})");
        let mut stream = tokio::select! {
            result = timeout(
                IMAP_REQUEST_TIMEOUT,
                session.uid_fetch(plan.uid.to_string(), query),
            ) => result
                .map_err(|_| Error::Other(format!("Timed out fetching body for UID {}", plan.uid)))??,
            _ = cancel_token.cancelled() => return Err(Error::ThreadCancel),
        };
        let mut fetched_parts = HashSet::new();

        while let Some(fetch) = tokio::select! {
            result = timeout(IMAP_REQUEST_TIMEOUT, stream.next()) => result
                .map_err(|_| Error::Other(format!("Timed out reading body for UID {}", plan.uid)))?,
            _ = cancel_token.cancelled() => return Err(Error::ThreadCancel),
        } {
            let fetch = fetch?;
            if fetch.uid != Some(plan.uid) {
                continue;
            }

            for part in &plan.body_parts {
                if let Some(body) = mime_part_text(&fetch, part) {
                    fetched_parts.insert(part.part_id.clone());
                    if part.html {
                        html_body.push(body);
                    } else {
                        text_body.push(body);
                    }
                }
            }
        }

        if fetched_parts.len() != plan.body_parts.len() {
            return Err(Error::Other(format!(
                "IMAP body response for UID {} was incomplete",
                plan.uid
            )));
        }
    }

    let content = EmailContent::from_header_and_parts(
        &plan.header,
        text_body.join("\n\n"),
        html_body.join("\n\n"),
        plan.attachments,
    );

    Ok(StandardEmail {
        uid: Some(plan.uid),
        date: plan.date,
        body: Some(encode_cached_email_content(&content)?),
        labels: plan.labels,
        read: plan.read,
    })
}

pub(crate) fn canonical_part_path(value: &str) -> Option<Vec<u32>> {
    let parts = value
        .split('.')
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    (!parts.is_empty() && parts.iter().all(|part| *part > 0)).then_some(parts)
}

pub(crate) fn mailbox_name_matches(mailbox: &MailBox, name: &str, delimiter: Option<&str>) -> bool {
    let expected = mailbox.as_ref();

    if name.eq_ignore_ascii_case(expected) {
        return true;
    }

    delimiter
        .and_then(|delimiter| name.rsplit_once(delimiter))
        .is_some_and(|(_, leaf)| leaf.eq_ignore_ascii_case(expected))
}

fn has_special_use_attribute(attributes: &[NameAttribute<'_>], mailbox: &MailBox) -> bool {
    attributes.iter().any(|attribute| {
        matches!(
            (mailbox, attribute),
            (MailBox::Sent, NameAttribute::Sent)
                | (MailBox::Drafts, NameAttribute::Drafts)
                | (MailBox::Trash, NameAttribute::Trash)
                | (MailBox::Spam, NameAttribute::Junk)
        )
    })
}

async fn resolve_imap_mailbox_name(
    session: &mut Session<TlsStream<TcpStream>>,
    mailbox: &MailBox,
) -> Result<String, Error> {
    if matches!(mailbox, MailBox::Inbox) {
        return Ok(mailbox.as_ref().to_string());
    }

    let mut special_use_match = None;
    let mut name_match = None;
    let mut names = session.list(Some(""), Some("*")).await?;

    while let Some(name) = names.next().await {
        let name = name?;

        if special_use_match.is_none() && has_special_use_attribute(name.attributes(), mailbox) {
            special_use_match = Some(name.name().to_string());
        }

        if name_match.is_none() && mailbox_name_matches(mailbox, name.name(), name.delimiter()) {
            name_match = Some(name.name().to_string());
        }
    }

    let resolved = special_use_match
        .or(name_match)
        .unwrap_or_else(|| mailbox.as_ref().to_string());

    if resolved != mailbox.as_ref() {
        tracing::info!(
            mailbox = mailbox.as_ref(),
            imap_mailbox = resolved,
            "Resolved logical mailbox to server mailbox"
        );
    }

    Ok(resolved)
}

async fn select_mailbox(
    session: &mut Session<TlsStream<TcpStream>>,
    mailbox: &MailBox,
) -> Result<async_imap::types::Mailbox, Error> {
    let imap_mailbox = resolve_imap_mailbox_name(session, mailbox).await?;
    Ok(session.select(imap_mailbox).await?)
}

#[tracing::instrument(
    name = "imap.email.set_read",
    skip(session),
    fields(mailbox = mailbox.as_ref(), uid, read),
    err
)]
pub async fn fetch_attachment_on_server(
    session: SharedImapSession,
    mailbox: &MailBox,
    uid: u32,
    part_id: &str,
) -> Result<FetchedAttachment, Error> {
    let path = canonical_part_path(part_id)
        .ok_or_else(|| Error::Other("Invalid attachment part identifier".to_string()))?;
    let canonical_id = render_part_id(&path);
    let query = format!("(UID BODY.PEEK[{canonical_id}.MIME] BODY.PEEK[{canonical_id}])");
    let mut session = session.lock().await;
    let mailbox_info = select_mailbox(&mut session, mailbox).await?;
    let uid_validity = mailbox_info.uid_validity.unwrap_or_default();
    let mut stream = session.uid_fetch(uid.to_string(), query).await?;

    while let Some(fetch) = stream.next().await {
        let fetch = fetch?;
        if fetch.uid != Some(uid) {
            continue;
        }

        let mime_path = SectionPath::Part(path.clone(), Some(MessageSection::Mime));
        let body_path = SectionPath::Part(path.clone(), None);
        let mime = fetch.section(&mime_path).ok_or(Error::NotFound)?;
        let body = fetch.section(&body_path).ok_or(Error::NotFound)?;
        let mut raw = mime.to_vec();
        if !raw.ends_with(b"\r\n\r\n") {
            if !raw.ends_with(b"\r\n") {
                raw.extend_from_slice(b"\r\n");
            }
            raw.extend_from_slice(b"\r\n");
        }
        raw.extend_from_slice(body);

        let message = mail_parser::MessageParser::default()
            .parse(&raw)
            .ok_or_else(|| Error::Other("Unable to decode attachment data".to_string()))?;
        let attachment = message
            .attachments()
            .next()
            .or_else(|| message.parts.first())
            .ok_or(Error::NotFound)?;
        return Ok(FetchedAttachment {
            data: attachment.contents().to_vec(),
            uid_validity,
        });
    }

    Err(Error::NotFound)
}

pub async fn set_email_read_on_server(
    session: SharedImapSession,
    mailbox: &MailBox,
    uid: u32,
    read: bool,
) -> Result<(), Error> {
    let mut session = session.lock().await;
    select_mailbox(&mut session, mailbox).await?;

    let flag_operation = if read {
        "+FLAGS.SILENT (\\Seen)"
    } else {
        "-FLAGS.SILENT (\\Seen)"
    };
    let mut updates = session.uid_store(uid.to_string(), flag_operation).await?;

    while let Some(update) = updates.next().await {
        update?;
    }

    Ok(())
}

async fn sync_email_read_states(
    session: &mut Session<TlsStream<TcpStream>>,
    pool: &SqlitePool,
    table_name: &str,
    cancel_token: &CancellationToken,
) -> Result<Vec<(u32, bool)>, Error> {
    let mut stream = tokio::select! {
        result = session.uid_fetch("1:*", "(UID FLAGS)") => result?,
        _ = cancel_token.cancelled() => return Err(Error::ThreadCancel),
    };
    let mut read_states = Vec::new();

    while let Some(message) = tokio::select! {
        message = stream.next() => message,
        _ = cancel_token.cancelled() => return Err(Error::ThreadCancel),
    } {
        let message = message?;
        if let Some(uid) = message.uid {
            let read = message
                .flags()
                .any(|flag| flag == async_imap::types::Flag::Seen);
            read_states.push((uid, read));
        }
    }

    set_email_read_states(pool, table_name, &read_states).await
}

#[tracing::instrument(
    name = "imap.sync.initial",
    skip_all,
    fields(provider = provider.as_ref())
)]
#[allow(clippy::too_many_arguments)]
pub async fn fetch_emails(
    session: SharedImapSession,
    background_session: Option<SharedImapSession>,
    pool: &SqlitePool,
    provider: Provider,
    cancel_token: CancellationToken,
    inbox_initial_email_populated_tx: Arc<Mutex<Option<Sender<()>>>>,
    sent_initial_email_populated_tx: Arc<Mutex<Option<Sender<()>>>>,
    spam_initial_email_populated_tx: Arc<Mutex<Option<Sender<()>>>>,
) -> Result<FetchResult, Error> {
    if cancel_token.is_cancelled() {
        return Err(Error::ThreadCancel);
    }

    let table_name = format!("{}_{}", provider.as_ref(), MailBox::Inbox.as_ref());

    if crate::db::check_email_db_empty(pool, &table_name).await?
        && get_last_uid(pool, &table_name).await?.is_some()
    {
        tracing::info!("Emails already fetched");
        return Ok(FetchResult::Populated);
    }

    let inbox_tx = inbox_initial_email_populated_tx.clone();
    let sent_tx = sent_initial_email_populated_tx.clone();
    let spam_tx = spam_initial_email_populated_tx.clone();

    let background_task = if let Some(background_session) = background_session {
        let pool = pool.clone();
        let provider = provider.clone();
        let token = cancel_token.child_token();
        let spam_token = token.child_token();
        let background_span = tracing::info_span!(
            "imap.sync.initial.background",
            provider = provider.as_ref(),
            mailbox = "Sent"
        );

        Some(tokio::spawn(
            async move {
                let sent_table_name = format!("{}_{}", provider.as_ref(), MailBox::Sent.as_ref());
                let spam_table_name = format!("{}_{}", provider.as_ref(), MailBox::Spam.as_ref());

                let sent_result = async {
                    let mailbox_info = {
                        let mut session = background_session.lock().await;
                        select_mailbox(&mut session, &MailBox::Sent).await?
                    };
                    populate_sent_folder_count(&pool, provider.as_ref(), mailbox_info.exists)
                        .await?;

                    handle_email_population_locked(
                        background_session.clone(),
                        &pool,
                        &MailBox::Sent,
                        sent_table_name,
                        token,
                        inbox_initial_email_populated_tx.clone(),
                        sent_initial_email_populated_tx.clone(),
                        spam_initial_email_populated_tx.clone(),
                    )
                    .await
                }
                .await;

                if let Err(error) = sent_result {
                    tracing::error!(?error, "Initial Sent mailbox sync failed");
                }

                let spam_result = handle_email_population_locked(
                    background_session,
                    &pool,
                    &MailBox::Spam,
                    spam_table_name,
                    spam_token,
                    inbox_initial_email_populated_tx,
                    sent_initial_email_populated_tx,
                    spam_initial_email_populated_tx.clone(),
                )
                .await;

                if let Err(error) = spam_result {
                    tracing::error!(?error, "Initial Spam mailbox sync failed");
                }

                if let Some(tx) = spam_initial_email_populated_tx.lock().await.take() {
                    _ = tx.send(());
                }

                Ok::<_, Error>(())
            }
            .instrument(background_span),
        ))
    } else {
        None
    };

    let mailbox_info = {
        let mut session = session.lock().await;
        select_mailbox(&mut session, &MailBox::Inbox).await?
    };

    populate_inbox_folder_count(pool, provider.as_ref(), mailbox_info.exists).await?;

    let result = handle_email_population_locked(
        session,
        pool,
        &MailBox::Inbox,
        table_name,
        cancel_token.clone(),
        inbox_tx,
        sent_tx,
        spam_tx,
    )
    .await?;

    if let Some(task) = background_task {
        match task.await {
            Ok(Ok(())) => {}
            Ok(Err(Error::ThreadCancel)) => {}
            Err(err) if err.is_cancelled() => {}
            Ok(Err(err)) => {
                tracing::error!(?err, "Background mailbox task failed");
            }
            Err(err) => {
                tracing::error!(?err, "Background mailbox task panicked");
            }
        }
    }

    Ok(result)
}

#[tracing::instrument(
    name = "imap.sync.initial.mailbox",
    skip_all,
    fields(mailbox = mailbox.as_ref(), table = table_name)
)]
#[allow(clippy::too_many_arguments)]
async fn handle_email_population_locked(
    session: SharedImapSession,
    pool: &SqlitePool,
    mailbox: &MailBox,
    table_name: String,
    cancel_token: CancellationToken,
    inbox_initial_email_populated_tx: Arc<Mutex<Option<Sender<()>>>>,
    sent_initial_email_populated_tx: Arc<Mutex<Option<Sender<()>>>>,
    spam_initial_email_populated_tx: Arc<Mutex<Option<Sender<()>>>>,
) -> Result<FetchResult, Error> {
    if cancel_token.is_cancelled() {
        return Err(Error::ThreadCancel);
    }

    let mut session = tokio::select! {
        guard = session.lock() => guard,

        _ = cancel_token.cancelled() => {
            return Err(Error::ThreadCancel);
        }
    };

    handle_email_population(
        &mut session,
        pool,
        mailbox,
        table_name,
        cancel_token,
        inbox_initial_email_populated_tx,
        sent_initial_email_populated_tx,
        spam_initial_email_populated_tx,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn handle_email_population(
    session: &mut Session<TlsStream<TcpStream>>,
    pool: &SqlitePool,
    mailbox: &MailBox,
    table_name: String,
    cancel_token: CancellationToken,
    inbox_initial_email_populated_tx: Arc<Mutex<Option<Sender<()>>>>,
    sent_initial_email_populated_tx: Arc<Mutex<Option<Sender<()>>>>,
    spam_initial_email_populated_tx: Arc<Mutex<Option<Sender<()>>>>,
) -> Result<FetchResult, Error> {
    if cancel_token.is_cancelled() {
        return Err(Error::ThreadCancel);
    }

    let mailbox_info = tokio::select! {
        result = select_mailbox(session, mailbox) => result?,

        _ = cancel_token.cancelled() => {
            return Err(Error::ThreadCancel);
        }
    };

    if let Some(uid_validity) = mailbox_info.uid_validity {
        reconcile_mailbox_uid_validity(pool, &table_name, uid_validity).await?;
    }

    let notify_initial_population = || async {
        match mailbox {
            MailBox::Inbox => {
                if let Some(tx) = inbox_initial_email_populated_tx.lock().await.take() {
                    _ = tx.send(());
                }
            }
            MailBox::Sent => {
                if let Some(tx) = sent_initial_email_populated_tx.lock().await.take() {
                    _ = tx.send(());
                }
            }
            MailBox::Spam => {
                if let Some(tx) = spam_initial_email_populated_tx.lock().await.take() {
                    _ = tx.send(());
                }
            }
            MailBox::Drafts | MailBox::Trash => {}
        }
    };

    let set_progress = |percentage: u8| async move {
        match mailbox {
            MailBox::Inbox => INBOX_POPULATE_UPDATE
                .0
                .lock()
                .await
                .store(percentage, std::sync::atomic::Ordering::Release),
            MailBox::Sent => SENT_POPULATE_UPDATE
                .0
                .lock()
                .await
                .store(percentage, std::sync::atomic::Ordering::Release),
            MailBox::Spam => SPAM_POPULATE_UPDATE
                .0
                .lock()
                .await
                .store(percentage, std::sync::atomic::Ordering::Release),
            MailBox::Drafts | MailBox::Trash => {}
        }
    };

    if mailbox_info.exists == 0 {
        set_progress(100).await;
        notify_initial_population().await;
        return Ok(FetchResult::EmptyMailbox);
    }

    // Search first, then fetch metadata in small newest-first chunks. This lets
    // the UI read the first useful page without waiting for a whole-mailbox
    // BODYSTRUCTURE scan and keeps progress tied to stored messages.
    let mut uids = tokio::select! {
        result = timeout(IMAP_REQUEST_TIMEOUT, session.uid_search("ALL")) => result
            .map_err(|_| Error::Other("Timed out listing mailbox UIDs".to_string()))??,
        _ = cancel_token.cancelled() => return Err(Error::ThreadCancel),
    }
    .into_iter()
    .collect::<Vec<_>>();
    uids.sort_unstable_by(|left, right| right.cmp(left));

    if uids.is_empty() {
        set_progress(100).await;
        notify_initial_population().await;
        return Ok(FetchResult::EmptyMailbox);
    }

    let highest_uid = uids.iter().copied().max().unwrap_or_default();
    let total_to_process = uids.len();
    let mut processed_count = 0usize;
    let mut inserted_count = 0u32;
    let mut initial_population_notified = false;

    for uid_chunk in uids.chunks(INITIAL_SYNC_CHUNK_SIZE) {
        let uid_set = uid_chunk
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let mut messages_stream = tokio::select! {
            result = timeout(
                IMAP_REQUEST_TIMEOUT,
                session.uid_fetch(uid_set, FETCH_MESSAGE_ATTRIBUTES),
            ) => result
                .map_err(|_| Error::Other("Timed out fetching message metadata".to_string()))??,
            _ = cancel_token.cancelled() => return Err(Error::ThreadCancel),
        };
        let mut plans = Vec::with_capacity(uid_chunk.len());

        while let Some(message) = tokio::select! {
            result = timeout(IMAP_REQUEST_TIMEOUT, messages_stream.next()) => result
                .map_err(|_| Error::Other("Timed out reading message metadata".to_string()))?,
            _ = cancel_token.cancelled() => return Err(Error::ThreadCancel),
        } {
            plans.push(message_fetch_plan(&message?)?);
        }
        drop(messages_stream);
        plans.sort_unstable_by(|left, right| right.uid.cmp(&left.uid));

        let mut emails = Vec::with_capacity(plans.len());
        for plan in plans {
            emails.push(fetch_message_without_attachments(session, plan, &cancel_token).await?);
            processed_count += 1;
            let percentage = ((processed_count * 100) / total_to_process) as u8;
            set_progress(percentage.min(99)).await;
        }

        if !emails.is_empty() {
            let batch_count = emails.len() as u32;
            if initial_population_notified {
                replace_storage(pool, emails, &table_name).await?;
            } else {
                // Atomically swap the preserved legacy cache for the first
                // usable selective-cache chunk, then append later chunks.
                reset_storage(pool, emails, &table_name).await?;
            }
            inserted_count += batch_count;

            if !initial_population_notified {
                notify_initial_population().await;
                initial_population_notified = true;
            }
        }
    }

    if !initial_population_notified {
        notify_initial_population().await;
    }

    if cancel_token.is_cancelled() {
        return Err(Error::ThreadCancel);
    }

    set_last_uid(pool, &table_name, highest_uid).await?;
    set_progress(100).await;

    tracing::info!(
        inserted = inserted_count,
        mail = ?mailbox,
        highest_uid,
        "Initial mailbox sync completed"
    );

    Ok(FetchResult::Fetched {
        count: inserted_count as u16,
        total_emails: mailbox_info.exists,
    })
}

pub(crate) fn uid_vec_to_set(uids: &HashSet<u32>) -> String {
    uids.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

pub(crate) fn incremental_uid_search_query(last_uid: u32) -> String {
    format!("UID {}:*", last_uid.saturating_add(1))
}

#[tracing::instrument(
    name = "imap.sync.incremental",
    skip_all,
    fields(provider = provider.as_ref(), mailbox = mailbox.as_ref())
)]
pub async fn fetch_latest(
    app: tauri::AppHandle,
    session: SharedImapSession,
    pool: &SqlitePool,
    mailbox: &MailBox,
    provider: &Provider,
    cancel_token: CancellationToken,
) -> Result<FetchResult, Error> {
    let table_name = format!("{}_{}", provider.as_ref(), mailbox.as_ref());
    let notification_session = crate::notifications::current_session(&app, provider);

    let mut session = tokio::select! {
        guard = session.lock() => guard,

        _ = cancel_token.cancelled() => {
            return Err(Error::ThreadCancel);
        }
    };

    let mailbox_info = tokio::select! {
        result = select_mailbox(&mut session, mailbox) => result?,

        _ = cancel_token.cancelled() => {
            return Err(Error::ThreadCancel);
        }
    };

    if let Some(uid_validity) = mailbox_info.uid_validity {
        reconcile_mailbox_uid_validity(pool, &table_name, uid_validity).await?;
    }

    let total_emails = mailbox_info.exists;

    let read_state_updates =
        sync_email_read_states(&mut session, pool, &table_name, &cancel_token).await?;

    let last_uid = get_last_uid(pool, &table_name).await?.unwrap_or(0);

    let search_query = incremental_uid_search_query(last_uid);

    let mut new_uids = session.uid_search(search_query).await?;
    new_uids.retain(|uid| *uid > last_uid);

    if new_uids.is_empty() {
        tracing::info!("Mailbox already up to date");

        if !read_state_updates.is_empty() {
            if let Err(error) = app.emit(
                "emails-updated",
                serde_json::json!({
                    "mailbox": mailbox.as_ref(),
                    "provider": provider.as_ref(),
                    "newEmailsCount": 0,
                    "readStateUpdates": read_state_updates
                        .iter()
                        .map(|(uid, read)| serde_json::json!({ "uid": uid, "read": read }))
                        .collect::<Vec<_>>(),
                }),
            ) {
                tracing::warn!(?error, "Failed to notify frontend about read-state updates");
            }
        }

        return Ok(FetchResult::Fetched {
            count: 0,
            total_emails,
        });
    }

    let uid_set = uid_vec_to_set(&new_uids);

    let mut stream = tokio::select! {
        result = session.uid_fetch(
            uid_set,
            FETCH_MESSAGE_ATTRIBUTES
        ) => result?,

        _ = cancel_token.cancelled() => {
            return Err(Error::ThreadCancel);
        }
    };

    let mut plans = Vec::new();
    let mut emails: Vec<StandardEmail> = vec![];
    let mut spam_uids = HashSet::new();

    let mut highest_uid = last_uid;
    let mut inserted_count = 0usize;
    let mut message_count = 0usize;

    while let Some(message) = tokio::select! {
        msg = stream.next() => msg,

        _ = cancel_token.cancelled() => {
            tracing::info!(
                mailbox = ?mailbox,
                "Mailbox sync cancelled"
            );

            return Err(Error::ThreadCancel);
        }
    } {
        message_count += 1;

        let message = match message {
            Ok(message) => message,
            Err(err) => {
                tracing::error!(
                    error = %err,
                    mailbox = ?mailbox,
                    message_index = message_count,
                    requested_uid_count = new_uids.len(),
                    "Failed to decode IMAP message during incremental mailbox sync"
                );
                return Err(err.into());
            }
        };

        if let Some(uid) = message.uid {
            highest_uid = highest_uid.max(uid);
        }

        plans.push(message_fetch_plan(&message)?);
    }

    // The metadata stream holds a mutable borrow of the IMAP session.
    drop(stream);

    for plan in plans {
        if cancel_token.is_cancelled() {
            return Err(Error::ThreadCancel);
        }

        let email = fetch_message_without_attachments(&mut session, plan, &cancel_token).await?;

        if let Some(body) = CompleteEmail::from(email.clone()).body {
            let rating =
                phishing_rater::rater(app.clone(), body.subject, body.email_from, body.text_body)
                    .await?;

            if *mailbox == MailBox::Inbox && rating.label == Label::Spam && rating.score > 0.8 {
                if let Some(uid) = email.uid {
                    spam_uids.insert(uid);
                    continue;
                }
            }
        }

        emails.push(email);
    }

    if !spam_uids.is_empty() {
        let spam_mailbox = resolve_imap_mailbox_name(&mut session, &MailBox::Spam).await?;
        let spam_uid_set = uid_vec_to_set(&spam_uids);

        session.uid_mv(spam_uid_set, &spam_mailbox).await?;
        tracing::info!(
            count = spam_uids.len(),
            destination = spam_mailbox,
            "Moved high-confidence spam messages out of Inbox"
        );
    }

    let notifications = if *mailbox == MailBox::Inbox {
        emails
            .iter()
            .filter_map(|email| {
                let uid = email.uid?;
                let email = CompleteEmail::from(email.clone());
                let subject = email
                    .body
                    .map(|body| body.subject)
                    .unwrap_or_else(|| "New Mail".to_string());
                Some((uid, subject))
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    if !emails.is_empty() {
        inserted_count += emails.len();

        populate_storage(pool, emails, &table_name).await?;
    }

    set_last_uid(pool, &table_name, highest_uid).await?;

    if inserted_count > 0 || !read_state_updates.is_empty() {
        if let Err(error) = app.emit(
            "emails-updated",
            serde_json::json!({
                "mailbox": mailbox.as_ref(),
                "provider": provider.as_ref(),
                "newEmailsCount": inserted_count,
                "readStateUpdates": read_state_updates
                    .iter()
                    .map(|(uid, read)| serde_json::json!({ "uid": uid, "read": read }))
                    .collect::<Vec<_>>(),
            }),
        ) {
            tracing::warn!(?error, "Failed to notify frontend about mailbox updates");
        }
    }

    // A click can arrive immediately: only publish notifications after the
    // message has committed to the local database.
    if !cancel_token.is_cancelled() {
        if let Some(session) = notification_session {
            for (uid, subject) in notifications {
                crate::notifications::show_new_mail(
                    &app,
                    session.clone(),
                    provider.clone(),
                    uid,
                    subject,
                );
            }
        }
    }

    Ok(FetchResult::Fetched {
        count: inserted_count as u16,
        total_emails,
    })
}
