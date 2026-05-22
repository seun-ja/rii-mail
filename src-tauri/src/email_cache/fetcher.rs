use async_imap::Session;
use async_native_tls::TlsStream;
use futures::StreamExt;
use sqlx::SqlitePool;
use tokio::net::TcpStream;

use crate::{db::populate_storage, email_cache::Email, error::Error};

#[tracing::instrument(name = "emails.fetch", skip(session, pool))]
pub async fn fetch_emails(
    session: &mut Session<TlsStream<TcpStream>>,
    size: u32,
    pool: &SqlitePool,
) -> Result<u16, Error> {
    let mailbox = session.select("INBOX").await?;

    let mut emails: Vec<Email> = Vec::new();
    if mailbox.exists == 0 {
        tracing::info!("No emails found");
        return Ok(0);
    }

    let end = mailbox.exists;
    let start = end.saturating_sub(size.saturating_sub(1)).max(1);

    let sequence_set = format!("{}:{}", start, end);

    let mut messages_stream = session.fetch(sequence_set, "BODY[]").await?;

    while let Some(email) = messages_stream.next().await {
        let email = email?;
        emails.push(email.into());
    }

    tracing::info!("{} email(s) found", emails.len());

    let emails_len = emails.len();

    populate_storage(pool, emails).await?;
    Ok(emails_len as u16)
}
