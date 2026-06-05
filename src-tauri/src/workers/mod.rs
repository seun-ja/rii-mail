use std::sync::Arc;

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

mod email_fetcher;
mod general;

pub use general::session_thread;

#[derive(Default)]
pub struct FetchManager {
    pub logout_token: Mutex<CancellationToken>,
    pub refresh_token: Mutex<CancellationToken>,
}

pub type SharedFetchManager = Arc<FetchManager>;
