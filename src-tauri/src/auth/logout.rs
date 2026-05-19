use async_imap::Session;
use async_native_tls::TlsStream;
use tokio::net::TcpStream;

use crate::error::Error;

pub async fn logout(mut session: Session<TlsStream<TcpStream>>) -> Result<(), Error> {
    session.logout().await?;
    Ok(())
}
