use std::path::PathBuf;

use chrono::{DateTime, FixedOffset};
use sqlx::{QueryBuilder, SqlitePool};
use tokio::fs;

use crate::{
    email_cache::{CompleteEmail, Email},
    error::Error,
};

#[derive(sqlx::FromRow)]
struct DbEmail {
    pub date: Option<DateTime<FixedOffset>>,
    pub body: Option<Vec<u8>>,
    pub labels: Option<String>,
}

#[tracing::instrument(name = "db.populate_storage", skip(pool, emails))]
pub async fn populate_storage(
    pool: &SqlitePool,
    emails: Vec<Email>,
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

    let mut tx = pool.begin().await?;

    let mut query_builder: QueryBuilder<'_, sqlx::Sqlite> = QueryBuilder::new(&format!(
        "INSERT INTO {} (uid, date, body, labels) ",
        table_name
    ));

    query_builder.push_values(emails, |mut b, email| {
        let uid = email.uid.unwrap_or_default() as i64;
        let labels = email.labels.map(|l| serde_json::to_string(&l).unwrap());

        b.push_bind(uid)
            .push_bind(email.date)
            .push_bind(email.body)
            .push_bind(labels);
    });

    query_builder.build().execute(&mut *tx).await?;

    tx.commit().await?;

    Ok(())
}

#[tracing::instrument(name = "db.get_emails", skip(pool))]
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

            Ok(Email {
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

#[tracing::instrument(name = "db.cleanup", skip(db_path))]
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

pub async fn check_email_db_empty(pool: &SqlitePool, table_name: &str) -> Result<bool, Error> {
    let result: (bool,) = sqlx::query_as(&format!("SELECT EXISTS (SELECT 1 FROM {})", table_name))
        .fetch_one(pool)
        .await?;

    Ok(result.0)
}

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
