use async_imap::Client;
use async_native_tls::TlsStream;
use tokio::net::TcpStream;

use crate::error::Error;

pub async fn init_imap_session(
    imap_server: &str,
    imap_port: u16,
) -> Result<Client<TlsStream<TcpStream>>, Error> {
    let imap_addr = (imap_server, imap_port);
    let tcp_stream = TcpStream::connect(imap_addr).await?;
    let tls = async_native_tls::TlsConnector::new();
    let tls_stream = tls.connect(imap_server, tcp_stream).await?;

    tracing::info!("IMAP connected to {}", imap_server);

    let client = async_imap::Client::new(tls_stream);

    Ok(client)
}
