use async_imap::{Client, Session};
use async_native_tls::TlsStream;
use tokio::net::TcpStream;

use crate::error::Error;

pub async fn login(
    login: &str,
    password: &str,
    client: Option<Client<TlsStream<TcpStream>>>,
) -> Result<Session<TlsStream<TcpStream>>, Error> {
    let imap_session = client
        .ok_or(Error::Other(
            "Failed to Initiate TlsStream Client".to_string(),
        ))?
        .login(login, password)
        .await
        .map_err(|e| e.0)?;

    Ok(imap_session)
}
