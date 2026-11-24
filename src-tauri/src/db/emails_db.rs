use std::path::PathBuf;

use chrono::{TimeZone, Utc};
use sqlx::{FromRow, SqlitePool};
use tokio::fs;

use crate::{
    email_cache::{CompleteEmail, EmailAttachment, EmailContent, StandardEmail},
    error::Error,
};

#[derive(FromRow)]
struct DbMessage {
    uid: i64,
    internal_date: Option<i64>,
    subject: Option<String>,
    preview: Option<String>,
    is_seen: bool,
    body_state: String,
    text_body: Option<String>,
    html_body: Option<String>,
    sender_name: Option<String>,
    sender_address: Option<String>,
    cc_json: Option<String>,
    labels_json: Option<String>,
    attachments_json: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct AttachmentRecord {
    pub part_db_id: i64,
    pub section: String,
    pub file_name: String,
    pub content_type: String,
}

#[derive(Debug, Clone, FromRow)]
pub struct CachedPartRecord {
    pub part_id: i64,
    pub relative_path: String,
    pub byte_size: i64,
}

fn split_legacy_mailbox_key(mailbox_key: &str) -> Result<(&str, &str), Error> {
    mailbox_key
        .split_once('_')
        .ok_or_else(|| Error::Other(format!("Invalid mailbox key: {mailbox_key}")))
}

fn role_for_server_name(server_name: &str) -> &'static str {
    match server_name.to_ascii_lowercase().as_str() {
        "inbox" => "inbox",
        "sent" => "sent",
        "drafts" => "drafts",
        "trash" => "trash",
        "spam" | "junk" => "spam",
        "archive" | "all mail" => "archive",
        _ => "custom",
    }
}

/// Creates or returns an account row. Authentication secrets remain outside SQLite.
pub async fn ensure_account(
    pool: &SqlitePool,
    email: &str,
    display_name: Option<&str>,
    provider: &str,
) -> Result<i64, Error> {
    sqlx::query(
        r#"
        INSERT INTO accounts (email, display_name, provider)
        VALUES (?, ?, ?)
        ON CONFLICT(provider, email) DO UPDATE SET
            display_name = COALESCE(excluded.display_name, accounts.display_name),
            updated_at = unixepoch()
        "#,
    )
    .bind(email)
    .bind(display_name)
    .bind(provider)
    .execute(pool)
    .await?;

    Ok(
        sqlx::query_scalar("SELECT id FROM accounts WHERE provider = ? AND email = ?")
            .bind(provider)
            .bind(email)
            .fetch_one(pool)
            .await?,
    )
}

/// Creates or returns a server mailbox for an account.
pub async fn ensure_mailbox(
    pool: &SqlitePool,
    account_id: i64,
    server_name: &str,
    display_name: &str,
    role: &str,
) -> Result<i64, Error> {
    sqlx::query(
        r#"
        INSERT INTO mailboxes (account_id, server_name, display_name, role)
        VALUES (?, ?, ?, ?)
        ON CONFLICT(account_id, server_name) DO UPDATE SET
            display_name = excluded.display_name,
            role = excluded.role,
            updated_at = unixepoch()
        "#,
    )
    .bind(account_id)
    .bind(server_name)
    .bind(display_name)
    .bind(role)
    .execute(pool)
    .await?;

    Ok(
        sqlx::query_scalar("SELECT id FROM mailboxes WHERE account_id = ? AND server_name = ?")
            .bind(account_id)
            .bind(server_name)
            .fetch_one(pool)
            .await?,
    )
}

// The sync layer still passes keys such as `gmail_INBOX`. Keep that boundary working
// while it is converted to pass mailbox IDs directly. No provider-specific tables exist.
async fn resolve_legacy_mailbox(pool: &SqlitePool, mailbox_key: &str) -> Result<(i64, i64), Error> {
    let (provider, server_name) = split_legacy_mailbox_key(mailbox_key)?;
    let compatibility_identity = format!("__riimail_{provider}_account__");
    let account_id = match sqlx::query_scalar::<_, i64>(
        "SELECT id FROM accounts WHERE provider = ? AND email NOT GLOB '__riimail_*_account__' ORDER BY id LIMIT 1",
    )
    .bind(provider)
    .fetch_optional(pool)
    .await?
    {
        Some(account_id) => account_id,
        None => ensure_account(pool, &compatibility_identity, None, provider).await?,
    };
    let mailbox_id = ensure_mailbox(
        pool,
        account_id,
        server_name,
        server_name,
        role_for_server_name(server_name),
    )
    .await?;
    Ok((account_id, mailbox_id))
}

#[tracing::instrument(
    name = "db.email.batch_insert",
    skip_all,
    fields(mailbox = mailbox_key, email_count = emails.len()),
    err
)]
pub async fn populate_storage(
    pool: &SqlitePool,
    emails: Vec<StandardEmail>,
    mailbox_key: &str,
) -> Result<(), Error> {
    if emails.is_empty() {
        return Ok(());
    }

    let (account_id, mailbox_id) = resolve_legacy_mailbox(pool, mailbox_key).await?;
    let mut transaction = pool.begin().await?;

    for email in emails {
        let uid = i64::from(email.uid.unwrap_or_default());
        let internal_date = email.date.map(|date| date.timestamp_millis());
        let size_bytes = email
            .body
            .as_ref()
            .and_then(|body| i64::try_from(body.len()).ok());
        let parsed_body = CompleteEmail::from(email.clone()).body;
        let subject = parsed_body.as_ref().map(|body| body.subject.as_str());
        let preview = parsed_body.as_ref().map(|body| body.preview.as_str());
        let fetched_state = if parsed_body.is_some() {
            "fetched"
        } else {
            "not_fetched"
        };

        let message_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO messages (
                mailbox_id, uid, internal_date, subject, preview, size_bytes,
                is_seen, structure_state, body_state
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(mailbox_id, uid) DO UPDATE SET
                is_seen = excluded.is_seen,
                updated_at = unixepoch()
            RETURNING id
            "#,
        )
        .bind(mailbox_id)
        .bind(uid)
        .bind(internal_date)
        .bind(subject)
        .bind(preview)
        .bind(size_bytes)
        .bind(email.read)
        .bind("not_fetched")
        .bind(fetched_state)
        .fetch_one(&mut *transaction)
        .await?;

        if let Some(body) = parsed_body {
            sqlx::query(
                r#"
                INSERT INTO message_content (message_id, text_body, html_body)
                VALUES (?, ?, ?)
                ON CONFLICT(message_id) DO NOTHING
                "#,
            )
            .bind(message_id)
            .bind(&body.text_body)
            .bind(&body.html_body)
            .execute(&mut *transaction)
            .await?;

            sqlx::query(
                r#"
                INSERT INTO message_addresses (message_id, kind, position, name, address)
                VALUES (?, 'from', 0, ?, ?)
                ON CONFLICT(message_id, kind, position) DO NOTHING
                "#,
            )
            .bind(message_id)
            .bind(&body.sender_name)
            .bind(&body.email_from)
            .execute(&mut *transaction)
            .await?;

            if let Some(cc) = body.cc {
                for (position, (name, address)) in cc.into_iter().enumerate() {
                    sqlx::query(
                        r#"
                        INSERT INTO message_addresses (
                            message_id, kind, position, name, address
                        )
                        VALUES (?, 'cc', ?, ?, ?)
                        ON CONFLICT(message_id, kind, position) DO NOTHING
                        "#,
                    )
                    .bind(message_id)
                    .bind(i64::try_from(position).unwrap_or(i64::MAX))
                    .bind(name)
                    .bind(address)
                    .execute(&mut *transaction)
                    .await?;
                }
            }

            // BODYSTRUCTURE metadata is small and safe to persist during sync;
            // the attachment payload itself remains on the IMAP server.
            sqlx::query("DELETE FROM mime_parts WHERE message_id = ?")
                .bind(message_id)
                .execute(&mut *transaction)
                .await?;
            for attachment in body.attachments {
                sqlx::query(
                    r#"
                    INSERT INTO mime_parts (
                        message_id, section, content_type, disposition,
                        filename, encoded_size, is_body_candidate
                    )
                    VALUES (?, ?, ?, 'attachment', ?, ?, 0)
                    "#,
                )
                .bind(message_id)
                .bind(attachment.part_id)
                .bind(attachment.content_type)
                .bind(attachment.file_name)
                .bind(i64::try_from(attachment.size).unwrap_or(i64::MAX))
                .execute(&mut *transaction)
                .await?;
            }
        }

        if let Some(labels) = email.labels {
            for label in labels {
                let label_id: i64 = sqlx::query_scalar(
                    r#"
                    INSERT INTO labels (account_id, name)
                    VALUES (?, ?)
                    ON CONFLICT(account_id, name) DO UPDATE SET name = excluded.name
                    RETURNING id
                    "#,
                )
                .bind(account_id)
                .bind(label)
                .fetch_one(&mut *transaction)
                .await?;

                sqlx::query(
                    "INSERT OR IGNORE INTO message_labels (message_id, label_id) VALUES (?, ?)",
                )
                .bind(message_id)
                .bind(label_id)
                .execute(&mut *transaction)
                .await?;
            }
        }
    }

    transaction.commit().await?;
    Ok(())
}

const MESSAGE_SELECT: &str = r#"
    SELECT
        m.uid,
        m.internal_date,
        m.subject,
        m.preview,
        m.is_seen,
        m.body_state,
        c.text_body,
        c.html_body,
        sender.name AS sender_name,
        sender.address AS sender_address,
        CASE WHEN EXISTS (
            SELECT 1 FROM message_addresses cc_exists
            WHERE cc_exists.message_id = m.id AND cc_exists.kind = 'cc'
        ) THEN (
            SELECT json_group_array(json_array(cc.name, cc.address))
            FROM message_addresses cc
            WHERE cc.message_id = m.id AND cc.kind = 'cc'
            ORDER BY cc.position
        ) END AS cc_json,
        CASE WHEN EXISTS (
            SELECT 1 FROM message_labels ml_exists
            WHERE ml_exists.message_id = m.id
        ) THEN (
            SELECT json_group_array(l.name)
            FROM message_labels ml
            JOIN labels l ON l.id = ml.label_id
            WHERE ml.message_id = m.id
        ) END AS labels_json,
        CASE WHEN EXISTS (
            SELECT 1 FROM mime_parts mp_exists
            WHERE mp_exists.message_id = m.id
              AND mp_exists.disposition = 'attachment'
        ) THEN (
            SELECT json_group_array(json_object(
                'id', mp.id,
                'partId', mp.section,
                'fileName', COALESCE(mp.filename, 'attachment'),
                'contentType', mp.content_type,
                'size', COALESCE(mp.decoded_size, mp.encoded_size, 0)
            ))
            FROM mime_parts mp
            WHERE mp.message_id = m.id
              AND mp.disposition = 'attachment'
            ORDER BY mp.section
        ) END AS attachments_json
    FROM messages m
    LEFT JOIN message_content c ON c.message_id = m.id
    LEFT JOIN message_addresses sender
        ON sender.message_id = m.id AND sender.kind = 'from' AND sender.position = 0
"#;

fn db_message_to_complete(row: DbMessage) -> CompleteEmail {
    let labels = row.labels_json.as_deref().and_then(|labels| {
        serde_json::from_str::<Vec<String>>(labels)
            .map_err(|error| {
                tracing::warn!(?error, "Skipping malformed message labels");
                error
            })
            .ok()
    });
    let cc = row
        .cc_json
        .as_deref()
        .and_then(|cc| serde_json::from_str::<Vec<(Option<String>, String)>>(cc).ok())
        .map(|addresses| {
            addresses
                .into_iter()
                .map(|(name, address)| (name.unwrap_or_default(), address))
                .collect()
        });
    let attachments = row
        .attachments_json
        .as_deref()
        .and_then(|attachments| serde_json::from_str::<Vec<EmailAttachment>>(attachments).ok())
        .unwrap_or_default();
    let body = (row.body_state == "fetched").then(|| EmailContent {
        subject: row.subject.unwrap_or_else(|| "No Subject".to_string()),
        sender_name: row
            .sender_name
            .unwrap_or_else(|| "Unknown Sender".to_string()),
        email_from: row
            .sender_address
            .unwrap_or_else(|| "unknown@local".to_string()),
        cc,
        preview: row
            .preview
            .unwrap_or_else(|| "No preview available".to_string()),
        html_body: row.html_body.unwrap_or_default(),
        text_body: row.text_body.unwrap_or_default(),
        attachments,
        parts: Vec::new(),
        raw_message: String::new(),
    });

    CompleteEmail {
        uid: u32::try_from(row.uid).ok(),
        date: row.internal_date.and_then(|timestamp| {
            Utc.timestamp_millis_opt(timestamp)
                .single()
                .map(|date| date.fixed_offset())
        }),
        body,
        labels,
        read: row.is_seen,
    }
}

#[tracing::instrument(
    name = "db.email.page",
    skip_all,
    fields(mailbox = mailbox_key, min_range, max_range),
    err
)]
pub async fn get_emails(
    pool: &SqlitePool,
    mailbox_key: &str,
    min_range: u32,
    max_range: u32,
) -> Result<Vec<CompleteEmail>, Error> {
    if max_range <= min_range {
        return Ok(Vec::new());
    }

    let (_, mailbox_id) = resolve_legacy_mailbox(pool, mailbox_key).await?;
    let query = format!(
        "{MESSAGE_SELECT} WHERE m.mailbox_id = ? \
         ORDER BY m.internal_date DESC, m.id DESC LIMIT ? OFFSET ?"
    );
    let rows = sqlx::query_as::<_, DbMessage>(&query)
        .bind(mailbox_id)
        .bind(i64::from(max_range - min_range))
        .bind(i64::from(min_range))
        .fetch_all(pool)
        .await?;

    Ok(rows.into_iter().map(db_message_to_complete).collect())
}

#[tracing::instrument(
    name = "db.email.by_uid",
    skip_all,
    fields(mailbox = mailbox_key, uid),
    err
)]
pub async fn get_email_by_uid(
    pool: &SqlitePool,
    mailbox_key: &str,
    uid: u32,
) -> Result<Option<CompleteEmail>, Error> {
    let (_, mailbox_id) = resolve_legacy_mailbox(pool, mailbox_key).await?;
    let query = format!("{MESSAGE_SELECT} WHERE m.mailbox_id = ? AND m.uid = ?");
    let row = sqlx::query_as::<_, DbMessage>(&query)
        .bind(mailbox_id)
        .bind(i64::from(uid))
        .fetch_optional(pool)
        .await?;
    Ok(row.map(db_message_to_complete))
}

#[tracing::instrument(
    name = "db.email.set_read",
    skip(pool),
    fields(mailbox = mailbox_key, uid, read),
    err
)]
pub async fn set_email_read(
    pool: &SqlitePool,
    mailbox_key: &str,
    uid: u32,
    read: bool,
) -> Result<(), Error> {
    let (_, mailbox_id) = resolve_legacy_mailbox(pool, mailbox_key).await?;
    sqlx::query(
        "UPDATE messages SET is_seen = ?, updated_at = unixepoch() WHERE mailbox_id = ? AND uid = ?",
    )
    .bind(read)
    .bind(mailbox_id)
    .bind(i64::from(uid))
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_email_read_states(
    pool: &SqlitePool,
    mailbox_key: &str,
    read_states: &[(u32, bool)],
) -> Result<Vec<(u32, bool)>, Error> {
    if read_states.is_empty() {
        return Ok(Vec::new());
    }
    let (_, mailbox_id) = resolve_legacy_mailbox(pool, mailbox_key).await?;
    let mut transaction = pool.begin().await?;
    let mut changed = Vec::new();

    for (uid, read) in read_states {
        let result = sqlx::query(
            r#"
            UPDATE messages
            SET is_seen = ?, updated_at = unixepoch()
            WHERE mailbox_id = ? AND uid = ? AND is_seen != ?
            "#,
        )
        .bind(read)
        .bind(mailbox_id)
        .bind(i64::from(*uid))
        .bind(read)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() > 0 {
            changed.push((*uid, *read));
        }
    }

    transaction.commit().await?;
    Ok(changed)
}

async fn set_mailbox_count(
    pool: &SqlitePool,
    provider: &str,
    server_name: &str,
    count: u32,
) -> Result<(), Error> {
    let mailbox_key = format!("{provider}_{server_name}");
    let (_, mailbox_id) = resolve_legacy_mailbox(pool, &mailbox_key).await?;
    sqlx::query(
        "UPDATE mailboxes SET server_message_count = ?, updated_at = unixepoch() WHERE id = ?",
    )
    .bind(i64::from(count))
    .bind(mailbox_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn populate_inbox_folder_count(
    pool: &SqlitePool,
    provider: &str,
    count: u32,
) -> Result<(), Error> {
    set_mailbox_count(pool, provider, "INBOX", count).await
}

pub async fn populate_sent_folder_count(
    pool: &SqlitePool,
    provider: &str,
    count: u32,
) -> Result<(), Error> {
    set_mailbox_count(pool, provider, "Sent", count).await
}

#[tracing::instrument(name = "db.cleanup", skip_all, err)]
pub async fn cleanup(db_path: PathBuf) -> Result<(), Error> {
    let metadata = match fs::metadata(&db_path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let result = if metadata.is_dir() {
        fs::remove_dir_all(&db_path).await
    } else {
        fs::remove_file(&db_path).await
    };
    match result {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
            ) =>
        {
            tracing::warn!(?error, path = %db_path.display(), "Skipping cleanup path removal");
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

pub async fn check_email_db_empty(pool: &SqlitePool, mailbox_key: &str) -> Result<bool, Error> {
    let (_, mailbox_id) = resolve_legacy_mailbox(pool, mailbox_key).await?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM messages WHERE mailbox_id = ?)")
            .bind(mailbox_id)
            .fetch_one(pool)
            .await?;
    Ok(exists)
}

pub async fn get_last_uid(pool: &SqlitePool, mailbox_key: &str) -> Result<Option<u32>, Error> {
    let (_, mailbox_id) = resolve_legacy_mailbox(pool, mailbox_key).await?;
    let uid: Option<i64> =
        sqlx::query_scalar("SELECT highest_uid_seen FROM mailboxes WHERE id = ?")
            .bind(mailbox_id)
            .fetch_one(pool)
            .await?;
    Ok(uid.and_then(|uid| u32::try_from(uid).ok()))
}

pub async fn set_last_uid(pool: &SqlitePool, mailbox_key: &str, uid: u32) -> Result<(), Error> {
    let (_, mailbox_id) = resolve_legacy_mailbox(pool, mailbox_key).await?;
    sqlx::query("UPDATE mailboxes SET highest_uid_seen = ?, updated_at = unixepoch() WHERE id = ?")
        .bind(i64::from(uid))
        .bind(mailbox_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn reconcile_mailbox_uid_validity(
    pool: &SqlitePool,
    mailbox_key: &str,
    uid_validity: u32,
) -> Result<bool, Error> {
    let (_, mailbox_id) = resolve_legacy_mailbox(pool, mailbox_key).await?;
    let previous: Option<i64> =
        sqlx::query_scalar("SELECT uid_validity FROM mailboxes WHERE id = ?")
            .bind(mailbox_id)
            .fetch_one(pool)
            .await?;
    let changed = previous.is_some_and(|value| value != i64::from(uid_validity));
    let mut transaction = pool.begin().await?;

    if changed {
        sqlx::query("DELETE FROM messages WHERE mailbox_id = ?")
            .bind(mailbox_id)
            .execute(&mut *transaction)
            .await?;
    }

    sqlx::query(
        "UPDATE mailboxes SET uid_validity = ?, highest_uid_seen = CASE WHEN ? THEN NULL ELSE highest_uid_seen END, updated_at = unixepoch() WHERE id = ?",
    )
    .bind(i64::from(uid_validity))
    .bind(changed)
    .bind(mailbox_id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    Ok(changed)
}

pub async fn get_mailbox_uid_validity(
    pool: &SqlitePool,
    mailbox_key: &str,
) -> Result<Option<u32>, Error> {
    let (_, mailbox_id) = resolve_legacy_mailbox(pool, mailbox_key).await?;
    let value: Option<i64> = sqlx::query_scalar("SELECT uid_validity FROM mailboxes WHERE id = ?")
        .bind(mailbox_id)
        .fetch_one(pool)
        .await?;
    Ok(value.and_then(|value| u32::try_from(value).ok()))
}

pub async fn get_attachment_record(
    pool: &SqlitePool,
    mailbox_key: &str,
    uid: u32,
    section: &str,
) -> Result<Option<AttachmentRecord>, Error> {
    let (_, mailbox_id) = resolve_legacy_mailbox(pool, mailbox_key).await?;
    Ok(sqlx::query_as::<_, AttachmentRecord>(
        r#"
        SELECT
            mp.id AS part_db_id,
            mp.section,
            COALESCE(mp.filename, 'attachment') AS file_name,
            mp.content_type
        FROM mime_parts mp
        JOIN messages m ON m.id = mp.message_id
        WHERE m.mailbox_id = ? AND m.uid = ? AND mp.section = ?
          AND mp.disposition = 'attachment'
        "#,
    )
    .bind(mailbox_id)
    .bind(i64::from(uid))
    .bind(section)
    .fetch_optional(pool)
    .await?)
}

pub async fn get_cached_part(
    pool: &SqlitePool,
    part_id: i64,
) -> Result<Option<CachedPartRecord>, Error> {
    let cached = sqlx::query_as::<_, CachedPartRecord>(
        "SELECT part_id, relative_path, byte_size FROM part_cache WHERE part_id = ? AND state = 'ready'",
    )
    .bind(part_id)
    .fetch_optional(pool)
    .await?;

    if cached.is_some() {
        sqlx::query("UPDATE part_cache SET last_accessed_at = unixepoch() WHERE part_id = ?")
            .bind(part_id)
            .execute(pool)
            .await?;
    }

    Ok(cached)
}

pub async fn store_cached_part(
    pool: &SqlitePool,
    part_id: i64,
    relative_path: &str,
    byte_size: usize,
) -> Result<(), Error> {
    sqlx::query(
        r#"
        INSERT INTO part_cache (part_id, relative_path, byte_size, state)
        VALUES (?, ?, ?, 'ready')
        ON CONFLICT(part_id) DO UPDATE SET
            relative_path = excluded.relative_path,
            byte_size = excluded.byte_size,
            state = 'ready',
            cached_at = unixepoch(),
            last_accessed_at = unixepoch()
        "#,
    )
    .bind(part_id)
    .bind(relative_path)
    .bind(i64::try_from(byte_size).unwrap_or(i64::MAX))
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn cached_parts_for_eviction(
    pool: &SqlitePool,
    protected_part_id: i64,
) -> Result<Vec<CachedPartRecord>, Error> {
    Ok(sqlx::query_as::<_, CachedPartRecord>(
        r#"
        SELECT part_id, relative_path, byte_size
        FROM part_cache
        WHERE state = 'ready' AND part_id != ?
        ORDER BY last_accessed_at ASC, cached_at ASC
        "#,
    )
    .bind(protected_part_id)
    .fetch_all(pool)
    .await?)
}

pub async fn cached_parts_size(pool: &SqlitePool) -> Result<i64, Error> {
    Ok(sqlx::query_scalar(
        "SELECT COALESCE(SUM(byte_size), 0) FROM part_cache WHERE state = 'ready'",
    )
    .fetch_one(pool)
    .await?)
}

pub async fn remove_cached_part(pool: &SqlitePool, part_id: i64) -> Result<(), Error> {
    sqlx::query("DELETE FROM part_cache WHERE part_id = ?")
        .bind(part_id)
        .execute(pool)
        .await?;
    Ok(())
}
