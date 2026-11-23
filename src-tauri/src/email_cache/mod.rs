use std::sync::Arc;

use async_imap::types::{Fetch, Flag};
use async_imap::Session;
use async_native_tls::TlsStream;
use chrono::{DateTime, FixedOffset};
use mail_parser::{Addr, Address, Message, MessageParser, MimeHeaders};
use serde::{Deserialize, Serialize};
use tokio::{net::TcpStream, sync::Mutex};

mod fetcher;

#[cfg(test)]
pub(crate) use fetcher::{canonical_part_path, collect_body_structure_parts};
pub use fetcher::{
    fetch_attachment_on_server, fetch_emails, fetch_latest, set_email_read_on_server,
};
#[cfg(test)]
pub(crate) use fetcher::{
    incremental_uid_search_query, mailbox_name_matches, uid_vec_to_set, FetchResult,
};
use sqlx::prelude::FromRow;

pub type SharedImapSession = Arc<Mutex<Session<TlsStream<TcpStream>>>>;

const STRUCTURED_EMAIL_PREFIX: &[u8] = b"RIIMAIL-CACHE-V1\0";

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
pub struct StandardEmail {
    pub uid: Option<u32>,
    pub date: Option<DateTime<FixedOffset>>,
    pub body: Option<Vec<u8>>,
    pub labels: Option<Vec<String>>,
    pub read: bool,
}

impl From<Fetch> for StandardEmail {
    fn from(fetch: Fetch) -> Self {
        let date = fetch
            .internal_date()
            .or_else(|| fetch.body().and_then(extract_date_from_raw_message));

        let body = fetch.body().map(|b| b.to_vec());

        let labels = fetch
            .gmail_labels()
            .map(|labels| labels.iter().map(|s| s.to_string()).collect());
        let read = fetch.flags().any(|flag| flag == Flag::Seen);

        Self {
            uid: fetch.uid,
            date,
            body,
            labels,
            read,
        }
    }
}

impl From<StandardEmail> for CompleteEmail {
    fn from(value: StandardEmail) -> Self {
        let (body, parsed_date) = match value.body.as_deref() {
            Some(raw) if raw.starts_with(STRUCTURED_EMAIL_PREFIX) => (
                serde_json::from_slice(&raw[STRUCTURED_EMAIL_PREFIX.len()..]).ok(),
                None,
            ),
            Some(raw) => {
                let parsed_message = MessageParser::default().parse(raw);
                let parsed_date = parsed_message.as_ref().and_then(extract_date_from_message);
                (parsed_message.map(EmailContent::from), parsed_date)
            }
            None => (None, None),
        };

        Self {
            uid: value.uid,
            date: value.date.or(parsed_date),
            body,
            labels: value.labels,
            read: value.read,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailAttachment {
    pub id: u32,
    #[serde(default)]
    pub part_id: String,
    pub file_name: String,
    pub content_type: String,
    pub size: usize,
    #[serde(skip)]
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteEmail {
    pub uid: Option<u32>,
    pub date: Option<DateTime<FixedOffset>>,
    pub body: Option<EmailContent>,
    pub labels: Option<Vec<String>>,
    #[serde(default)]
    pub read: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailContent {
    pub subject: String,
    pub sender_name: String,
    pub email_from: String,
    #[serde(default)]
    pub cc: Option<Vec<(String, String)>>,
    pub preview: String,
    pub html_body: String,
    pub text_body: String,
    pub attachments: Vec<EmailAttachment>,
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

fn address_parts(address: &Addr<'_>) -> Option<(String, String)> {
    let email = address.address.as_deref()?.trim();
    if email.is_empty() {
        return None;
    }

    let name = address
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or_default()
        .to_string();

    Some((name, email.to_string()))
}

fn extract_sender(address: Option<&Address<'_>>) -> Option<(String, String)> {
    let (name, email) = address?.iter().find_map(address_parts)?;
    let sender_name = if name.is_empty() {
        email
            .split('@')
            .next()
            .filter(|local_part| !local_part.is_empty())
            .unwrap_or("Unknown Sender")
            .to_string()
    } else {
        name
    };

    Some((sender_name, email))
}

fn extract_addresses(address: Option<&Address<'_>>) -> Option<Vec<(String, String)>> {
    let addresses = address?
        .iter()
        .filter_map(address_parts)
        .collect::<Vec<_>>();

    (!addresses.is_empty()).then_some(addresses)
}

pub(crate) fn normalize_attachment_file_name(name: Option<&str>, attachment_id: u32) -> String {
    let file_name = name
        .and_then(|name| name.rsplit(['/', '\\']).next())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| {
            name.chars()
                .filter(|character| !character.is_control())
                .collect()
        })
        .filter(|name: &String| !name.is_empty() && name != "." && name != "..");

    file_name.unwrap_or_else(|| format!("attachment-{}", attachment_id + 1))
}

fn attachment_content_type(part: &mail_parser::MessagePart<'_>) -> String {
    part.content_type()
        .map(|content_type| {
            content_type.subtype().map_or_else(
                || content_type.ctype().to_string(),
                |subtype| format!("{}/{}", content_type.ctype(), subtype),
            )
        })
        .filter(|content_type| !content_type.trim().is_empty())
        .unwrap_or_else(|| "application/octet-stream".to_string())
}

fn extract_attachments(message: &Message<'_>) -> Vec<EmailAttachment> {
    message
        .attachments()
        .enumerate()
        .map(|(index, part)| {
            let id = index as u32;
            let data = part.contents().to_vec();

            EmailAttachment {
                id,
                part_id: (id + 1).to_string(),
                file_name: normalize_attachment_file_name(part.attachment_name(), id),
                content_type: attachment_content_type(part),
                size: data.len(),
                data,
            }
        })
        .collect()
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

pub(crate) fn encode_cached_email_content(
    content: &EmailContent,
) -> Result<Vec<u8>, serde_json::Error> {
    let encoded = serde_json::to_vec(content)?;
    let mut body = Vec::with_capacity(STRUCTURED_EMAIL_PREFIX.len() + encoded.len());
    body.extend_from_slice(STRUCTURED_EMAIL_PREFIX);
    body.extend_from_slice(&encoded);
    Ok(body)
}

impl EmailContent {
    pub(crate) fn from_header_and_parts(
        header: &[u8],
        text_body: String,
        html_body: String,
        attachments: Vec<EmailAttachment>,
    ) -> Self {
        let mut header_message = header.to_vec();
        if !header_message.ends_with(b"\r\n\r\n") {
            if !header_message.ends_with(b"\r\n") {
                header_message.extend_from_slice(b"\r\n");
            }
            header_message.extend_from_slice(b"\r\n");
        }
        let parsed_header = MessageParser::default().parse(&header_message);
        let subject = parsed_header
            .as_ref()
            .and_then(|message| message.subject())
            .unwrap_or("No Subject")
            .to_string();
        let (sender_name, email_from) = parsed_header
            .as_ref()
            .and_then(|message| extract_sender(message.from()))
            .unwrap_or_else(|| ("Unknown Sender".to_string(), "unknown@local".to_string()));
        let cc = parsed_header
            .as_ref()
            .and_then(|message| extract_addresses(message.cc()));
        let preview_source = if text_body.trim().is_empty() {
            html_body.as_str()
        } else {
            text_body.as_str()
        };
        let preview = normalize_text(preview_source, 120);

        Self {
            subject,
            sender_name,
            email_from,
            cc,
            preview,
            html_body,
            text_body,
            attachments,
            parts: Vec::new(),
            raw_message: String::new(),
        }
    }
}

impl CompleteEmail {
    pub fn into_frontend(self, id: String) -> FrontendEmail {
        let labels = self.labels.unwrap_or_default();
        let (folder, starred) = map_folder_and_starred(&labels);

        let (
            subject,
            sender_name,
            email_from,
            cc,
            preview,
            body,
            text_body,
            html_body,
            attachments,
        ) = match self.body {
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
                    content.cc,
                    normalize_text(&content.preview, 120),
                    normalize_text(&body, 2000),
                    content.text_body,
                    content.html_body,
                    content.attachments,
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
            uid: self.uid,
            folder,
            sender_name,
            email_from,
            cc,
            subject,
            preview,
            body,
            text_body,
            html_body,
            attachments,
            time,
            starred,
            read: self.read,
        }
    }
}

impl<'a> From<Message<'a>> for EmailContent {
    fn from(message: Message<'a>) -> Self {
        let subject = message.subject().unwrap_or("No Subject").to_string();
        let (sender_name, email_from) = extract_sender(message.from())
            .unwrap_or_else(|| ("Unknown Sender".to_string(), "unknown@local".to_string()));
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

        let attachments = extract_attachments(&message);
        let parts = (0..message.parts.len() as u32).collect();
        let raw_message = String::from_utf8_lossy(message.raw_message()).into_owned();

        let cc = extract_addresses(message.cc());

        Self {
            subject,
            sender_name,
            email_from,
            cc,
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
    pub uid: Option<u32>,
    pub folder: String,
    pub sender_name: String,
    pub email_from: String,
    pub cc: Option<Vec<(String, String)>>,
    pub subject: String,
    pub preview: String,
    pub body: String,
    pub text_body: String,
    pub html_body: String,
    pub attachments: Vec<EmailAttachment>,
    pub time: String,
    pub starred: bool,
    pub read: bool,
}
