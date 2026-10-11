use std::path::PathBuf;

use chrono::{DateTime, FixedOffset};
use sqlx::{QueryBuilder, SqlitePool};
use tokio::fs;

use crate::{
    email_cache::{CompleteEmail, StandardEmail},
    error::Error,
};

#[derive(sqlx::FromRow)]
struct DbEmail {
    pub date: Option<DateTime<FixedOffset>>,
    pub body: Option<Vec<u8>>,
    pub labels: Option<String>,
}

#[tracing::instrument(
    name = "db.email.batch_insert",
    skip_all,
    fields(table = table_name, email_count = emails.len()),
    err
)]
pub async fn populate_storage(
    pool: &SqlitePool,
    emails: Vec<StandardEmail>,
    table_name: &str,
) -> Result<(), Error> {
    if emails.is_empty() {
        return Ok(());
    }

    let mut emails = emails;
    emails.sort_by(|a, b| match (&a.date, &b.date) {
        (Some(a_date), Some(b_date)) => b_date.cmp(a_date),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });

    let emails = emails
        .into_iter()
        .map(|email| {
            let labels = email
                .labels
                .map(|labels| serde_json::to_string(&labels))
                .transpose()?;

            Ok((
                email.uid.unwrap_or_default() as i64,
                email.date,
                email.body,
                labels,
            ))
        })
        .collect::<Result<Vec<_>, serde_json::Error>>()?;

    let mut tx = pool.begin().await?;

    let mut query_builder: QueryBuilder<'_, sqlx::Sqlite> = QueryBuilder::new(&format!(
        "INSERT OR IGNORE INTO {} (uid, date, body, labels) ",
        table_name
    ));

    query_builder.push_values(emails, |mut b, (uid, date, body, labels)| {
        b.push_bind(uid)
            .push_bind(date)
            .push_bind(body)
            .push_bind(labels);
    });

    query_builder.build().execute(&mut *tx).await?;

    tx.commit().await?;

    Ok(())
}

#[tracing::instrument(
    name = "db.email.page",
    skip_all,
    fields(table = table_name, min_range, max_range),
    err
)]
// could use some streaming?
pub async fn get_emails(
    pool: &SqlitePool,
    table_name: &str,
    min_range: u16,
    max_range: u16,
) -> Result<Vec<CompleteEmail>, Error> {
    if max_range <= min_range {
        return Ok(Vec::new());
    }

    let limit = i64::from(max_range - min_range);
    let offset = i64::from(min_range);

    let rows: Vec<DbEmail> = sqlx::query_as::<_, DbEmail>(&format!(
        "SELECT date, body, labels FROM {} ORDER BY datetime(date) DESC, rowid DESC LIMIT ? OFFSET ?",
        table_name
    ))
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    let emails = rows
        .into_iter()
        .map(|row| {
            let labels = row.labels.and_then(|s| match serde_json::from_str(&s) {
                Ok(parsed) => Some(parsed),
                Err(err) => {
                    ::tracing::warn!(error = ?err, labels = %s, "Skipping malformed labels for email row");
                    None
                }
            });

            Ok(StandardEmail {
                uid: None,
                date: row.date,
                body: row.body,
                labels,
            })
        })
        .collect::<Result<Vec<_>, serde_json::Error>>()?;

    let complete_emails = emails.into_iter().map(CompleteEmail::from).collect();

    Ok(complete_emails)
}

#[tracing::instrument(
    name = "db.folder_count.upsert",
    skip_all,
    fields(provider, mailbox = "INBOX", count),
    err
)]
pub async fn populate_inbox_folder_count(
    pool: &SqlitePool,
    provider: &str,
    count: u32,
) -> Result<(), Error> {
    sqlx::query(
        r#"
        INSERT INTO total_emails (provider, INBOX)
        VALUES (?, ?)
        ON CONFLICT(provider)
        DO UPDATE SET INBOX = excluded.INBOX
        "#,
    )
    .bind(provider)
    .bind(count)
    .execute(pool)
    .await?;

    Ok(())
}

#[tracing::instrument(
    name = "db.folder_count.upsert",
    skip_all,
    fields(provider, mailbox = "Sent", count),
    err
)]
pub async fn populate_sent_folder_count(
    pool: &SqlitePool,
    provider: &str,
    count: u32,
) -> Result<(), Error> {
    sqlx::query(
        r#"
        INSERT INTO total_emails (provider, Sent)
        VALUES (?, ?)
        ON CONFLICT(provider)
        DO UPDATE SET Sent = excluded.Sent
        "#,
    )
    .bind(provider)
    .bind(count)
    .execute(pool)
    .await?;

    Ok(())
}

#[tracing::instrument(name = "db.cleanup", skip_all, err)]
pub async fn cleanup(db_path: PathBuf) -> Result<(), Error> {
    let metadata = match fs::metadata(&db_path).await {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };

    let deletion_result = if metadata.is_dir() {
        fs::remove_dir_all(&db_path).await
    } else {
        fs::remove_file(&db_path).await
    };

    if let Err(err) = deletion_result {
        if matches!(
            err.kind(),
            std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
        ) {
            ::tracing::warn!(
                error = ?err,
                path = %db_path.display(),
                "Skipping cleanup path removal"
            );
            return Ok(());
        }

        return Err(err.into());
    }

    Ok(())
}

#[tracing::instrument(
    name = "db.email.exists",
    skip_all,
    fields(table = table_name),
    err
)]
pub async fn check_email_db_empty(pool: &SqlitePool, table_name: &str) -> Result<bool, Error> {
    let result: (bool,) = sqlx::query_as(&format!("SELECT EXISTS (SELECT 1 FROM {})", table_name))
        .fetch_one(pool)
        .await?;

    Ok(result.0)
}

#[tracing::instrument(name = "db.sync_state.read", skip_all, fields(mailbox), err)]
pub async fn get_last_uid(pool: &SqlitePool, mailbox: &str) -> Result<Option<u32>, Error> {
    let result: Option<(i64,)> = sqlx::query_as(
        r#"
        SELECT last_uid
        FROM mailbox_sync_state
        WHERE mailbox = ?
        "#,
    )
    .bind(mailbox)
    .fetch_optional(pool)
    .await?;

    Ok(result.map(|r| r.0 as u32))
}

#[tracing::instrument(
    name = "db.sync_state.write",
    skip_all,
    fields(mailbox, last_uid = uid),
    err
)]
pub async fn set_last_uid(pool: &SqlitePool, mailbox: &str, uid: u32) -> Result<(), Error> {
    sqlx::query(
        r#"
        INSERT INTO mailbox_sync_state (
            mailbox,
            last_uid
        )
        VALUES (?, ?)
        ON CONFLICT(mailbox)
        DO UPDATE SET
            last_uid = excluded.last_uid
        "#,
    )
    .bind(mailbox)
    .bind(uid as i64)
    .execute(pool)
    .await?;

    Ok(())
}
