use async_imap::Client;
use async_native_tls::TlsStream;
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

use crate::{db, error::Error};

#[derive(Debug, Clone, Copy)]
pub struct RefreshSummary {
    pub new_emails_count: u16,
    pub total_emails: u32,
}

#[derive(Debug)]
pub enum ImapCommand {
    Logout,
    FetchEmails(db::MailBox, db::Provider, CancellationToken),
    RefreshEmails {
        mail_box: db::MailBox,
        provider: db::Provider,
        response_channel: tokio::sync::oneshot::Sender<Result<RefreshSummary, String>>,
    },
}

pub async fn init_imap_client(
    imap_server_url: &str,
    imap_port: u16,
) -> Result<Client<TlsStream<TcpStream>>, Error> {
    let imap_addr = (imap_server_url, imap_port);
    let tcp_stream = TcpStream::connect(imap_addr).await?;
    let tls = async_native_tls::TlsConnector::new().danger_accept_invalid_certs(false);
    let tls_stream = tls.connect(imap_server_url, tcp_stream).await?;

    tracing::info!("IMAP connected to {}", imap_server_url);

    let client = async_imap::Client::new(tls_stream);

    Ok(client)
}
