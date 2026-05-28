use std::collections::HashSet;

use async_imap::Session;
use async_native_tls::TlsStream;
use futures::StreamExt;
use sqlx::SqlitePool;
use tokio::net::TcpStream;

use crate::{
    db::{get_last_uid, populate_storage, set_last_uid, MailBox, Providers},
    email_cache::Email,
    error::Error,
};

pub enum FetchResult {
    EmptyMailbox,
    Populated,
    Fetched(u16),
}

impl FetchResult {
    pub fn count(&self) -> u16 {
        match self {
            FetchResult::EmptyMailbox => 0,
            FetchResult::Populated => 0,
            FetchResult::Fetched(count) => *count,
        }
    }
}

const INITIAL_BATCH_SIZE: usize = 50;

#[tracing::instrument(name = "emails.fetch", skip(session, pool))]
pub async fn fetch_emails(
    session: &mut Session<TlsStream<TcpStream>>,
    pool: &SqlitePool,
    mailbox: &MailBox,
    provider: &Providers,
) -> Result<FetchResult, Error> {
    let table_name = format!("{}_{}", provider.as_ref(), mailbox.as_ref());
    if crate::db::check_email_db_empty(pool, &table_name).await? {
        tracing::info!("Emails Already Fetched");
        return Ok(FetchResult::Populated);
    }

    let mailbox = session.select(mailbox).await?;

    if mailbox.exists == 0 {
        tracing::info!("No emails found");
        return Ok(FetchResult::EmptyMailbox);
    }

    let mut emails: Vec<Email> = Vec::new();

    let all_uids = session.uid_search("ALL").await?;
    let uid_set = uid_vec_to_set(&all_uids);

    // TODO: Share with UI
    let _ = mailbox.exists;

    let mut messages_stream = session
        .uid_fetch(uid_set, "(UID FLAGS ENVELOPE INTERNALDATE BODY.PEEK[])")
        .await?;

    let mut inserted_count = 0usize;
    let mut highest_uid = 0u32;

    tracing::info!(
        "Fetching {} emails from IMAP server for {:?}...",
        mailbox.exists,
        provider
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

    Ok(FetchResult::Fetched(emails_len as u16))
}

pub(crate) fn uid_vec_to_set(uids: &HashSet<u32>) -> String {
    uids.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

#[tracing::instrument(name = "emails.fetch.latest", skip(session, pool))]
pub async fn fetch_latest(
    session: &mut Session<TlsStream<TcpStream>>,
    pool: &SqlitePool,
    mailbox: &MailBox,
    provider: &Providers,
) -> Result<FetchResult, Error> {
    let table_name = format!("{}_{}", provider.as_ref(), mailbox.as_ref());

    let mailbox = session.select(mailbox).await?;

    // TODO: Share with UI
    let _total_emails = mailbox.exists;

    let last_uid = get_last_uid(pool, &table_name).await?.unwrap_or(0);

    let search_query = format!("{}:*", last_uid + 1);

    let new_uids = session.uid_search(search_query).await?;

    if new_uids.is_empty() {
        tracing::info!("Mailbox already up to date");

        return Ok(FetchResult::Fetched(0));
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

    Ok(FetchResult::Fetched(inserted_count as u16))
}
