use std::sync::Arc;

use tokio::sync::{mpsc::UnboundedSender, Mutex};
use tokio_util::sync::CancellationToken;

use crate::{db::Provider, error::Error, imap::ImapCommand};

mod imap_session;

pub use imap_session::worker;
#[cfg(test)]
pub(crate) use imap_session::{cleanup_logout_resources, is_imap_connection_error};

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

/// Reset cancellation state for an authenticated session and queue its initial
/// mailbox import. Both interactive login and returning-user startup must use
/// this path so an authenticated UI never opens against an unpopulated DB.
pub async fn queue_initial_fetch(
    fetch_manager: &FetchManager,
    imap_cmd_channel_tx: &UnboundedSender<ImapCommand>,
    provider: Provider,
) -> Result<(), Error> {
    fetch_manager.reset_for_session().await;
    let fetch_token = fetch_manager.logout_token.lock().await.child_token();
    imap_cmd_channel_tx.send(ImapCommand::FetchEmails(provider, fetch_token))?;

    Ok(())
}
