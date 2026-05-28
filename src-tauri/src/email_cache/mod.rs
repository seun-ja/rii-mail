use async_imap::types::Fetch;
use chrono::{DateTime, FixedOffset};
use mail_parser::{Address, Message, MessageParser};
use serde::{Deserialize, Serialize};

mod fetcher;

pub use fetcher::{fetch_emails, fetch_latest};
#[cfg(test)]
pub(crate) use fetcher::{uid_vec_to_set, FetchResult};
use sqlx::prelude::FromRow;

fn extract_date_from_message(message: &Message<'_>) -> Option<DateTime<FixedOffset>> {
    message
        .date()
        .map(|date| date.to_rfc3339())
        .and_then(|date| DateTime::parse_from_rfc3339(&date).ok())
}

fn extract_date_from_raw_message(raw: &[u8]) -> Option<DateTime<FixedOffset>> {
    MessageParser::default()
        .parse(raw)
        .as_ref()
        .and_then(extract_date_from_message)
}

#[derive(FromRow, Clone)]
pub struct Email {
    pub uid: Option<u32>,
    pub date: Option<DateTime<FixedOffset>>,
    pub body: Option<Vec<u8>>,
    pub labels: Option<Vec<String>>,
}

impl From<Fetch> for Email {
    fn from(fetch: Fetch) -> Self {
        let date = fetch
            .internal_date()
            .or_else(|| fetch.body().and_then(extract_date_from_raw_message));

        let body = fetch.body().map(|b| b.to_vec());

        let labels = fetch
            .gmail_labels()
            .map(|labels| labels.iter().map(|s| s.to_string()).collect());

        Self {
            uid: fetch.uid,
            date,
            body,
            labels,
        }
    }
}

impl From<Email> for CompleteEmail {
    fn from(value: Email) -> Self {
        let parsed_message = value
            .body
            .as_deref()
            .and_then(|raw| MessageParser::default().parse(raw));

        let parsed_date = parsed_message.as_ref().and_then(extract_date_from_message);
        let body = parsed_message.map(EmailContent::from);

        Self {
            date: value.date.or(parsed_date),
            body,
            labels: value.labels,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteEmail {
    pub date: Option<DateTime<FixedOffset>>,
    pub body: Option<EmailContent>,
    pub labels: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailContent {
    pub subject: String,
    pub sender_name: String,
    pub email_from: String,
    pub preview: String,
    pub html_body: String,
    pub text_body: String,
    pub attachments: Vec<u32>,
    pub parts: Vec<u32>,
    pub raw_message: String,
}

pub(crate) fn normalize_text(value: &str, max_chars: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");

    if normalized.chars().count() <= max_chars {
        return normalized;
    }

    normalized.chars().take(max_chars).collect::<String>() + "..."
}

fn extract_first_from(address: Option<&Address<'_>>) -> (String, String) {
    match address {
        Some(Address::List(list)) => {
            if let Some(first) = list.first() {
                let sender_name = first
                    .name
                    .as_ref()
                    .map(|name| name.to_string())
                    .filter(|name| !name.trim().is_empty())
                    .or_else(|| {
                        first.address.as_ref().map(|addr| {
                            addr.split('@')
                                .next()
                                .unwrap_or("Unknown Sender")
                                .to_string()
                        })
                    })
                    .unwrap_or_else(|| "Unknown Sender".to_string());

                let email_from = first
                    .address
                    .as_ref()
                    .map(|addr| addr.to_string())
                    .unwrap_or_else(|| "unknown@local".to_string());

                (sender_name, email_from)
            } else {
                ("Unknown Sender".to_string(), "unknown@local".to_string())
            }
        }
        _ => ("Unknown Sender".to_string(), "unknown@local".to_string()),
    }
}

pub(crate) fn map_folder_and_starred(labels: &[String]) -> (String, bool) {
    let mut folder = "inbox".to_string();
    let mut starred = false;

    for label in labels {
        let lower = label.to_ascii_lowercase();

        if lower.contains("starred") {
            starred = true;
        }

        if lower.contains("sent") {
            folder = "sent".to_string();
        } else if lower.contains("archive") || lower.contains("junk") || lower.contains("spam") {
            folder = "archive".to_string();
        } else if lower.contains("inbox") {
            folder = "inbox".to_string();
        }
    }

    (folder, starred)
}

impl CompleteEmail {
    pub fn into_frontend(self, id: String) -> FrontendEmail {
        let labels = self.labels.unwrap_or_default();
        let (folder, starred) = map_folder_and_starred(&labels);

        let (subject, sender_name, email_from, preview, body, text_body, html_body) =
            match self.body {
                Some(content) => {
                    let body = match (
                        content.text_body.trim().is_empty(),
                        content.html_body.trim().is_empty(),
                    ) {
                        (false, _) => content.text_body.clone(),
                        (true, false) => content.html_body.clone(),
                        _ => "No message body".to_string(),
                    };

                    (
                        normalize_text(&content.subject, 160),
                        content.sender_name,
                        content.email_from,
                        normalize_text(&content.preview, 120),
                        normalize_text(&body, 2000),
                        content.text_body,
                        content.html_body,
                    )
                }
                None => Default::default(),
            };

        let time = self
            .date
            .map(|d| d.format("%b %d %H:%M").to_string())
            .unwrap_or_else(|| "Unknown".to_string());

        FrontendEmail {
            id,
            folder,
            sender_name,
            email_from,
            subject,
            preview,
            body,
            text_body,
            html_body,
            time,
            starred,
            read: false,
        }
    }
}

impl<'a> From<Message<'a>> for EmailContent {
    fn from(message: Message<'a>) -> Self {
        let subject = message.subject().unwrap_or("No Subject").to_string();
        let (sender_name, email_from) = extract_first_from(message.from());
        let preview = message
            .body_preview(120)
            .map(|p| p.to_string())
            .unwrap_or_else(|| "No preview available".to_string());

        let html_body = (0..message.html_body_count())
            .filter_map(|idx| message.body_html(idx).map(|body| body.into_owned()))
            .collect::<Vec<_>>()
            .join("\n\n");

        let text_body = (0..message.text_body_count())
            .filter_map(|idx| message.body_text(idx).map(|body| body.into_owned()))
            .collect::<Vec<_>>()
            .join("\n\n");

        let attachments = message.attachments.clone();
        let parts = (0..message.parts.len() as u32).collect();
        let raw_message = String::from_utf8_lossy(message.raw_message()).into_owned();

        Self {
            subject,
            sender_name,
            email_from,
            preview,
            html_body,
            text_body,
            attachments,
            parts,
            raw_message,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendEmail {
    pub id: String,
    pub folder: String,
    pub sender_name: String,
    pub email_from: String,
    pub subject: String,
    pub preview: String,
    pub body: String,
    pub text_body: String,
    pub html_body: String,
    pub time: String,
    pub starred: bool,
    pub read: bool,
}
