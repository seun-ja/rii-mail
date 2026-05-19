use async_imap::types::Fetch;
use chrono::{DateTime, FixedOffset};

mod fetcher;

pub use fetcher::fetch_emails;

pub struct FetchedMessages {
    pub date: Option<DateTime<FixedOffset>>,
    pub body: Option<Vec<u8>>,
    pub labels: Option<Vec<String>>,
}

impl From<Fetch> for FetchedMessages {
    fn from(value: Fetch) -> Self {
        FetchedMessages {
            date: value.internal_date(),
            labels: value
                .gmail_labels()
                .map(|labels| labels.iter().map(|s| s.to_string()).collect()),
            body: value.body().map(|b| b.to_vec()),
        }
    }
}
