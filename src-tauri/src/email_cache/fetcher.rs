use std::{collections::HashSet, sync::Arc};

use async_imap::{types::NameAttribute, Session};
use async_native_tls::TlsStream;
use futures::StreamExt;
use sqlx::SqlitePool;
use tokio::{
    net::TcpStream,
    sync::{oneshot::Sender, Mutex},
};
use tokio_util::sync::CancellationToken;
use tracing::Instrument as _;

use crate::{
    db::{
        get_last_uid, populate_inbox_folder_count, populate_sent_folder_count, populate_storage,
        set_last_uid, MailBox, Provider,
    },
    email_cache::{SharedImapSession, StandardEmail},
    error::Error,
    INBOX_POPULATE_UPDATE, SENT_POPULATE_UPDATE,
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

const INITIAL_BATCH_SIZE: u32 = 100;
const FETCH_MESSAGE_ATTRIBUTES: &str = "(UID FLAGS BODY.PEEK[])";

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
    inbox_intial_email_populated_tx: Arc<Mutex<Option<Sender<()>>>>,
    sent_intial_email_populated_tx: Arc<Mutex<Option<Sender<()>>>>,
) -> Result<FetchResult, Error> {
    if cancel_token.is_cancelled() {
        return Err(Error::ThreadCancel);
    }

    let table_name = format!("{}_{}", provider.as_ref(), MailBox::Inbox.as_ref());

    if crate::db::check_email_db_empty(pool, &table_name).await? {
        tracing::info!("Emails already fetched");
        return Ok(FetchResult::Populated);
    }

    let inbox_tx = inbox_intial_email_populated_tx.clone();
    let sent_tx = sent_intial_email_populated_tx.clone();

    let background_task = if let Some(background_session) = background_session {
        let pool = pool.clone();
        let provider = provider.clone();
        let token = cancel_token.child_token();
        let background_span = tracing::info_span!(
            "imap.sync.initial.background",
            provider = provider.as_ref(),
            mailbox = "Sent"
        );

        Some(tokio::spawn(
            async move {
                let table_name = format!("{}_{}", provider.as_ref(), MailBox::Sent.as_ref());

                let mailbox_info = {
                    let mut session = background_session.lock().await;
                    select_mailbox(&mut session, &MailBox::Sent).await?
                };

                populate_sent_folder_count(&pool, provider.as_ref(), mailbox_info.exists).await?;

                _ = handle_email_population_locked(
                    background_session,
                    &pool,
                    &MailBox::Sent,
                    table_name,
                    token,
                    inbox_intial_email_populated_tx,
                    sent_intial_email_populated_tx,
                )
                .await?;

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
async fn handle_email_population_locked(
    session: SharedImapSession,
    pool: &SqlitePool,
    mailbox: &MailBox,
    table_name: String,
    cancel_token: CancellationToken,
    inbox_intial_email_populated_tx: Arc<Mutex<Option<Sender<()>>>>,
    sent_intial_email_populated_tx: Arc<Mutex<Option<Sender<()>>>>,
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
        inbox_intial_email_populated_tx,
        sent_intial_email_populated_tx,
    )
    .await
}

async fn handle_email_population(
    session: &mut Session<TlsStream<TcpStream>>,
    pool: &SqlitePool,
    mailbox: &MailBox,
    table_name: String,
    cancel_token: CancellationToken,
    inbox_intial_email_populated_tx: Arc<Mutex<Option<Sender<()>>>>,
    sent_intial_email_populated_tx: Arc<Mutex<Option<Sender<()>>>>,
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

    if mailbox_info.exists == 0 {
        return Ok(FetchResult::EmptyMailbox);
    }

    let mut emails = Vec::new();

    tracing::info!("Starting uid_fetch");

    let mut messages_stream = tokio::select! {
        result = session.uid_fetch(
            "1:*",
            FETCH_MESSAGE_ATTRIBUTES
        ) => result?,

        _ = cancel_token.cancelled() => {
            return Err(Error::ThreadCancel);
        }
    };

    tracing::info!(mailbox = ?mailbox, "uid_fetch established, starting to stream messages");

    let mut inserted_count = 0u32;
    let mut highest_uid = 0u32;

    let mut email_count: u32 = 0;

    let sent_tx = sent_intial_email_populated_tx.clone();
    let inbox_tx = inbox_intial_email_populated_tx.clone();

    tracing::info!(
        mailbox = ?mailbox,
        total_emails = mailbox_info.exists,
        "Beginning to stream messages"
    );

    while let Some(message) = tokio::select! {
        msg = messages_stream.next() => msg,

        _ = cancel_token.cancelled() => {
            tracing::info!(
                mailbox = ?mailbox,
                "Mailbox sync cancelled"
            );

            return Err(Error::ThreadCancel);
        }
    } {
        email_count += 1;
        let percentage = ((email_count * 100) / mailbox_info.exists) as u8;

        if mailbox == &MailBox::Inbox {
            INBOX_POPULATE_UPDATE
                .0
                .lock()
                .await
                .store(percentage, std::sync::atomic::Ordering::Release);
        } else if mailbox == &MailBox::Sent {
            SENT_POPULATE_UPDATE
                .0
                .lock()
                .await
                .store(percentage, std::sync::atomic::Ordering::Release);
        }

        let message = match message {
            Ok(message) => message,
            Err(err) => {
                tracing::error!(
                    error = %err,
                    mailbox = ?mailbox,
                    message_index = email_count,
                    total_emails = mailbox_info.exists,
                    "Failed to decode IMAP message during initial mailbox sync"
                );
                return Err(err.into());
            }
        };

        if let Some(uid) = message.uid {
            highest_uid = highest_uid.max(uid);
        }

        emails.push(message.into());

        let mut initial_batch = INITIAL_BATCH_SIZE;

        if mailbox_info.exists < INITIAL_BATCH_SIZE {
            initial_batch = mailbox_info.exists;
        }

        if emails.len() == initial_batch as usize {
            if cancel_token.is_cancelled() {
                return Err(Error::ThreadCancel);
            }

            tokio::select! {
                result = populate_storage(
                    pool,
                    std::mem::take(&mut emails),
                    &table_name
                ) => {
                    result?;
                }

                _ = cancel_token.cancelled() => {
                    return Err(Error::ThreadCancel);
                }
            }

            match mailbox {
                MailBox::Inbox => {
                    if let Some(tx) = inbox_tx.lock().await.take() {
                        tracing::info!(mailbox = ?mailbox, "Inbox initial population complete, notifying UI");
                        _ = tx.send(());
                    }
                }
                MailBox::Sent => {
                    if let Some(tx) = sent_tx.lock().await.take() {
                        tracing::info!(mailbox = ?mailbox, "Sent initial population complete, notifying UI");
                        _ = tx.send(())
                    }
                }
                MailBox::Drafts => unimplemented!("Coming soon"),
                MailBox::Trash => unimplemented!("Coming soon"),
            }

            inserted_count += INITIAL_BATCH_SIZE;
        }
    }

    if !emails.is_empty() {
        if cancel_token.is_cancelled() {
            return Err(Error::ThreadCancel);
        }

        inserted_count += emails.len() as u32;

        populate_storage(pool, emails, &table_name).await?;
    }

    if cancel_token.is_cancelled() {
        return Err(Error::ThreadCancel);
    }

    set_last_uid(pool, &table_name, highest_uid).await?;

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
    session: SharedImapSession,
    pool: &SqlitePool,
    mailbox: &MailBox,
    provider: &Provider,
    cancel_token: CancellationToken,
) -> Result<FetchResult, Error> {
    let table_name = format!("{}_{}", provider.as_ref(), mailbox.as_ref());

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

    let total_emails = mailbox_info.exists;

    let last_uid = get_last_uid(pool, &table_name).await?.unwrap_or(0);

    let search_query = incremental_uid_search_query(last_uid);

    let mut new_uids = session.uid_search(search_query).await?;
    new_uids.retain(|uid| *uid > last_uid);

    if new_uids.is_empty() {
        tracing::info!("Mailbox already up to date");

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

    let mut emails: Vec<StandardEmail> = Vec::new();

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

        let email: StandardEmail = message.into();

        emails.push(email);
    }

    if !emails.is_empty() {
        inserted_count += emails.len();

        populate_storage(pool, emails, &table_name).await?;
    }

    set_last_uid(pool, &table_name, highest_uid).await?;

    Ok(FetchResult::Fetched {
        count: inserted_count as u16,
        total_emails,
    })
}
