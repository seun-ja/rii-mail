use std::sync::Arc;

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

mod imap_session;

#[cfg(test)]
pub(crate) use imap_session::is_imap_connection_error;
pub use imap_session::worker;

#[derive(Default)]
pub struct FetchManager {
    pub logout_token: Mutex<CancellationToken>,
    pub refresh_token: Mutex<CancellationToken>,
}

impl FetchManager {
    /// Starts a new authenticated session with uncancelled token parents.
    pub async fn reset_for_session(&self) {
        *self.logout_token.lock().await = CancellationToken::new();
        *self.refresh_token.lock().await = CancellationToken::new();
    }
}

pub type SharedFetchManager = Arc<FetchManager>;
