use std::sync::Arc;

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

mod imap_session;

pub use imap_session::worker;

#[derive(Default)]
pub struct FetchManager {
    pub logout_token: Mutex<CancellationToken>,
    pub refresh_token: Mutex<CancellationToken>,
}

pub type SharedFetchManager = Arc<FetchManager>;
