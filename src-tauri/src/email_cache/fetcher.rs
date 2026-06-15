use std::{collections::HashSet, sync::Arc};

use async_imap::Session;
use async_native_tls::TlsStream;
use futures::StreamExt;
use sqlx::SqlitePool;
use tokio::{
    net::TcpStream,
    sync::{oneshot::Sender, Mutex},
};
use tokio_util::sync::CancellationToken;

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

#[tracing::instrument(
    name = "emails.fetch",
    skip(
        session,
        pool,
        background_session,
        cancel_token,
        inbox_intial_email_populated_tx,
        sent_intial_email_populated_tx
    )
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

        Some(tokio::spawn(async move {
            let table_name = format!("{}_{}", provider.as_ref(), MailBox::Sent.as_ref());

            let mailbox_info = background_session
                .lock()
                .await
                .select(&MailBox::Sent)
                .await?;

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
        }))
    } else {
        None
    };

    let mailbox_info = session.lock().await.select(&MailBox::Inbox).await?;

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
    name = "handler.populate_emails",
    skip(
        session,
        pool,
        mailbox,
        table_name,
        cancel_token,
        inbox_intial_email_populated_tx,
        sent_intial_email_populated_tx
    )
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
        result = session.select(&mailbox) => result?,

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

#[tracing::instrument(name = "emails.fetch.latest", skip(session, pool))]
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

    let mailbox = tokio::select! {
        result = session.select(&mailbox) => result?,

        _ = cancel_token.cancelled() => {
            return Err(Error::ThreadCancel);
        }
    };

    let total_emails = mailbox.exists;

    let last_uid = get_last_uid(pool, &table_name).await?.unwrap_or(0);

    let search_query = format!("{}:*", last_uid + 1);

    let new_uids = session.uid_search(search_query).await?;

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
