use async_imap::Session;
use async_native_tls::TlsStream;
use futures::StreamExt;
use sqlx::SqlitePool;
use tokio::net::TcpStream;

use crate::{
    db::{populate_storage, MailBox, Providers},
    email_cache::Email,
    error::Error,
};

pub enum FetchResult {
    EmptyMailbox,
    Populated,
    Fetched(u16),
}

#[tracing::instrument(name = "emails.fetch", skip(session, pool))]
pub async fn fetch_emails(
    session: &mut Session<TlsStream<TcpStream>>,
    size: u32,
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

    let mut emails: Vec<Email> = Vec::new();
    if mailbox.exists == 0 {
        tracing::info!("No emails found");
        return Ok(FetchResult::EmptyMailbox);
    }

    let end = mailbox.exists;
    let start = end.saturating_sub(size.saturating_sub(1)).max(1);

    let sequence_set = format!("{}:{}", start, end);

    let mut messages_stream = session.fetch(sequence_set, "BODY[]").await?;
    // let mut messages_stream = session.fetch(sequence_set, "INTERNALDATE BODY[]").await?;

    tracing::info!("Fetching emails from IMAP server...");
    while let Some(email) = messages_stream.next().await {
        let email = email?;
        emails.push(email.into());

        if emails.len() == 50 {
            populate_storage(pool, emails.clone(), &table_name).await?;
            emails.clear();
        }
    }

    tracing::info!("{} email(s) found", emails.len());

    let emails_len = emails.len();

    populate_storage(pool, emails, &table_name).await?;
    Ok(FetchResult::Fetched(emails_len as u16))
}
