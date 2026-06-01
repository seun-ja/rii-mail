use std::collections::HashSet;

use async_imap::Session;
use async_native_tls::TlsStream;
use futures::StreamExt;
use sqlx::SqlitePool;
use tokio::net::TcpStream;

use crate::{
    db::{
        get_last_uid, populate_inbox_folder_count, populate_sent_folder_count, populate_storage,
        set_last_uid, MailBox, Providers,
    },
    email_cache::{Email, SharedImapSession},
    error::Error,
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

const INITIAL_BATCH_SIZE: usize = 100;

#[tracing::instrument(name = "emails.fetch", skip(session, pool, background_session))]
pub async fn fetch_emails(
    session: SharedImapSession,
    background_session: Option<SharedImapSession>,
    pool: &SqlitePool,
    mailbox: MailBox,
    provider: Providers,
) -> Result<FetchResult, Error> {
    let table_name = format!("{}_{}", provider.as_ref(), mailbox.as_ref());
    if crate::db::check_email_db_empty(pool, &table_name).await? {
        tracing::info!("Emails Already Fetched");
        return Ok(FetchResult::Populated);
    }

    let background_task = if !matches!(mailbox, MailBox::Sent) {
        background_session.map(|session_background| {
            let pool = pool.clone();
            let provider = provider.clone();

            tokio::spawn(async move {
                let sent_table_name = format!("{}_{}", provider.as_ref(), MailBox::Sent.as_ref());

                match handle_email_population_locked (
                    session_background.clone(),
                    &pool,
                    MailBox::Sent,
                    sent_table_name.clone(),
                ).await {
                    Ok(res) => {
                        let _ = populate_sent_folder_count(&pool, provider.as_ref(), res.count() as u32).await.unwrap_or_else(|err| {
                            tracing::error!(error = ?err, "Failed to populate sent folder count after background fetch");
                        });
                    },
                    Err(err) => tracing::error!(error = ?err, "Failed background sent mailbox fetch")
                }
            })
        })
    } else {
        None
    };

    let result =
        handle_email_population_locked(session, &pool, MailBox::Inbox, table_name).await.map( async |res| {
            let _ = populate_inbox_folder_count(&pool, provider.as_ref(), res.count() as u32).await.unwrap_or_else(|err| {
                    tracing::error!(error = ?err, "Failed to populate sent folder count after background fetch");
                });
            res
        })?.await;

    if let Some(background_task) = background_task {
        if let Err(err) = background_task.await {
            tracing::error!(error = ?err, "Background sent mailbox task join failed");
        }
    }

    Ok(result)
}

async fn handle_email_population_locked(
    session: SharedImapSession,
    pool: &SqlitePool,
    mailbox: MailBox,
    table_name: String,
) -> Result<FetchResult, Error> {
    let mut session = session.lock().await;

    handle_email_population(&mut session, pool, mailbox, table_name).await
}

async fn handle_email_population(
    session: &mut Session<TlsStream<TcpStream>>,
    pool: &SqlitePool,
    mailbox: MailBox,
    table_name: String,
) -> Result<FetchResult, Error> {
    let mailbox = session.select(&mailbox).await?;

    if mailbox.exists == 0 {
        tracing::info!("No emails found");
        return Ok(FetchResult::EmptyMailbox);
    }

    let mut emails: Vec<Email> = Vec::new();

    let mut messages_stream = session
        .uid_fetch("1:*", "(UID FLAGS ENVELOPE INTERNALDATE BODY.PEEK[])")
        .await?;

    let mut inserted_count = 0usize;
    let mut highest_uid = 0u32;

    tracing::info!(
        "Fetching {} emails from IMAP server from {table_name}...",
        mailbox.exists,
    );

    while let Some(email) = messages_stream.next().await {
        let email = email?;

        if let Some(uid) = email.uid {
            highest_uid = highest_uid.max(uid);
        }

        emails.push(email.into());

        if emails.len() == INITIAL_BATCH_SIZE {
            inserted_count += emails.len();

            populate_storage(pool, std::mem::take(&mut emails), &table_name).await?;
        }
    }

    tracing::info!("{} email(s) found", emails.len());

    let emails_len = emails.len();

    if !emails.is_empty() {
        inserted_count += emails.len();

        populate_storage(pool, emails, &table_name).await?;
    }

    set_last_uid(pool, &table_name, highest_uid).await?;

    tracing::info!(
        inserted = inserted_count,
        highest_uid,
        "Initial mailbox sync completed"
    );

    Ok(FetchResult::Fetched {
        count: emails_len as u16,
        total_emails: mailbox.exists,
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
    mailbox: MailBox,
    provider: Providers,
) -> Result<FetchResult, Error> {
    let table_name = format!("{}_{}", provider.as_ref(), mailbox.as_ref());

    let mut session = session.lock().await;

    let mailbox = session.select(&mailbox).await?;

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

    let mut stream = session
        .uid_fetch(uid_set, "(UID FLAGS ENVELOPE INTERNALDATE BODY.PEEK[])")
        .await?;

    let mut emails: Vec<Email> = Vec::new();

    let mut highest_uid = last_uid;
    let mut inserted_count = 0usize;

    while let Some(message) = stream.next().await {
        let message = message?;

        if let Some(uid) = message.uid {
            highest_uid = highest_uid.max(uid);
        }

        let email: Email = message.into();

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
