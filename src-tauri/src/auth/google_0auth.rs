use async_imap::{Authenticator, Client, Session};
use async_native_tls::TlsStream;
use tokio::net::TcpStream;

use crate::error::Error;

struct GoogleOAuth<'a> {
    access_token: &'a str,
    user: &'a str,
}

impl<'a> Authenticator for GoogleOAuth<'a> {
    type Response = String;

    fn process(&mut self, _challenge: &[u8]) -> Self::Response {
        format!(
            "user={}\x01auth=Bearer {}\x01\x01",
            self.user, self.access_token
        )
    }
}

pub async fn google_oauth(
    user: &str,
    access_token: &str,
    auth_type: &str,
    client: Client<TlsStream<TcpStream>>,
) -> Result<Session<TlsStream<TcpStream>>, Error> {
    let auth = GoogleOAuth { access_token, user };

    let imap_session = client
        .authenticate(auth_type, auth)
        .await
        .map_err(|e| e.0)?;

    Ok(imap_session)
}
