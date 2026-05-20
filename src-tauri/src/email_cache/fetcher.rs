use async_imap::Session;
use async_native_tls::TlsStream;
use futures::StreamExt;
use sqlx::SqlitePool;
use tokio::net::TcpStream;

use crate::{db::populate_storage, email_cache::Email, error::Error};

pub async fn fetch_emails(
    session: &mut Session<TlsStream<TcpStream>>,
    size: u32,
    pool: SqlitePool,
) -> Result<u16, Error> {
    let mut messages_stream = session.fetch(size.to_string(), "RFC822").await?;

    let mut emails: Vec<Email> = Vec::new();
    let emails_len = emails.len();

    let stream = messages_stream.next().await;

    if let Some(email) = stream {
        let e = email?;
        emails.push(e.into());
    }

    populate_storage(pool, emails).await?;
    Ok(emails_len as u16)
}
