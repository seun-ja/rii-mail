use async_imap::Session;
use async_native_tls::TlsStream;
use futures::StreamExt;
use tokio::net::TcpStream;

use crate::{email_cache::FetchedMessages, error::Error};

pub async fn fetch_emails(
    session: &mut Session<TlsStream<TcpStream>>,
    size: u32,
) -> Result<Vec<FetchedMessages>, Error> {
    let mut messages_stream = session.fetch(size.to_string(), "RFC822").await?;

    let mut emails: Vec<FetchedMessages> = Vec::new();

    let stream = messages_stream.next().await;

    if let Some(email) = stream {
        let e = email?;
        emails.push(e.into());
    }

    // TODO: save to database
    Ok(emails)
}
