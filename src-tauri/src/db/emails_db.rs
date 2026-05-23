use chrono::{DateTime, FixedOffset};
use sqlx::{QueryBuilder, SqlitePool};

use crate::{email_cache::Email, error::Error};

#[derive(sqlx::FromRow)]
struct DbEmail {
    pub date: Option<DateTime<FixedOffset>>,
    pub body: Option<Vec<u8>>,
    pub labels: Option<String>,
}

#[tracing::instrument(name = "db.populate_storage", skip(pool, emails))]
pub async fn populate_storage(pool: &SqlitePool, emails: Vec<Email>) -> Result<(), Error> {
    if emails.is_empty() {
        return Ok(());
    }

    let mut tx = pool.begin().await?;

    let mut query_builder: QueryBuilder<'_, sqlx::Sqlite> =
        QueryBuilder::new("INSERT INTO emails (date, body, labels) ");

    query_builder.push_values(emails, |mut b, email| {
        let labels = email.labels.map(|l| serde_json::to_string(&l).unwrap());

        b.push_bind(email.date)
            .push_bind(email.body)
            .push_bind(labels);
    });

    query_builder.build().execute(&mut *tx).await?;

    tx.commit().await?;

    Ok(())
}

#[tracing::instrument(name = "db.get_emails", skip(pool))]
// could use some streaming?
pub async fn _get_emails(
    pool: SqlitePool,
    _min_range: u64,
    _max_range: u64,
) -> Result<Vec<Email>, Error> {
    let rows: Vec<DbEmail> = sqlx::query_as::<_, DbEmail>("SELECT date, body, labels FROM emails")
        .fetch_all(&pool)
        .await?;

    let emails = rows
        .into_iter()
        .map(|row| {
            Ok(Email {
                date: row.date,
                body: row.body,
                labels: row.labels.map(|s| serde_json::from_str(&s)).transpose()?,
            })
        })
        .collect::<Result<Vec<_>, serde_json::Error>>()?;

    Ok(emails)
}

#[tracing::instrument(name = "db.cleanup", skip(pool))]
pub async fn cleanup(pool: &SqlitePool) -> Result<(), Error> {
    sqlx::query("DELETE FROM emails").execute(pool).await?;

    Ok(())
}

pub async fn check_email_db_empty(pool: &SqlitePool) -> Result<bool, Error> {
    let result: (bool,) = sqlx::query_as("SELECT EXISTS (SELECT 1 FROM emails)")
        .fetch_one(pool)
        .await?;

    Ok(result.0)
}
