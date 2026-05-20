use async_imap::types::Fetch;
use chrono::{DateTime, FixedOffset};

mod fetcher;

pub use fetcher::fetch_emails;
use sqlx::prelude::FromRow;

#[derive(FromRow)]
pub struct Email {
    pub date: Option<DateTime<FixedOffset>>,
    pub body: Option<Vec<u8>>,
    pub labels: Option<Vec<String>>,
}

impl From<Fetch> for Email {
    fn from(fetch: Fetch) -> Self {
        let date = fetch.internal_date();

        let body = fetch.body().map(|b| b.to_vec());

        let labels = fetch
            .gmail_labels()
            .map(|labels| labels.iter().map(|s| s.to_string()).collect());

        Self { date, body, labels }
    }
}
