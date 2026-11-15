mod emails_db;

use std::{fs, path::PathBuf};

pub use emails_db::{
    check_email_db_empty, cleanup, get_email_by_uid, get_emails, get_last_uid,
    get_mailbox_uid_validity, populate_inbox_folder_count, populate_sent_folder_count,
    populate_storage, reconcile_mailbox_uid_validity, replace_storage, reset_storage,
    set_email_read, set_email_read_states, set_last_uid, set_mailbox_uid_validity,
};

use sqlx::{sqlite::SqliteConnectOptions, SqlitePool};

use crate::error::Error;

#[tracing::instrument(
    name = "db.connect",
    skip_all,
    fields(database = db),
    err
)]
pub async fn db_pool(app_data_dir: &PathBuf, db: &str) -> Result<SqlitePool, sqlx::Error> {
    fs::create_dir_all(app_data_dir)?;

    let data_dir = app_data_dir.join("data");
    fs::create_dir_all(&data_dir)?;

    let db_path = data_dir.join(db);

    let options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true);

    SqlitePool::connect_with(options).await
}

#[tracing::instrument(
    name = "db.initialize",
    skip_all,
    fields(database = db, provider = provider.as_ref()),
    err
)]
pub async fn init_db(
    app_data_dir: &PathBuf,
    db: &str,
    provider: &Provider,
) -> Result<SqlitePool, Error> {
    let pool = db_pool(app_data_dir, db).await?;

    sqlx::query("PRAGMA journal_mode = WAL;")
        .execute(&pool)
        .await?;

    sqlx::query("PRAGMA synchronous = NORMAL;")
        .execute(&pool)
        .await?;

    sqlx::query("PRAGMA foreign_keys = ON;")
        .execute(&pool)
        .await?;

    create_table_for_provider(&pool, provider).await?;

    Ok(pool)
}

async fn create_table_for_provider(
    pool: &SqlitePool,
    provider: &Provider,
) -> Result<(), sqlx::Error> {
    let inbox_table_name = format!("{}_{}", provider.as_ref(), MailBox::Inbox.as_ref());
    let sent_table_name = format!("{}_{}", provider.as_ref(), MailBox::Sent.as_ref());
    let spam_table_name = format!("{}_{}", provider.as_ref(), MailBox::Spam.as_ref());

    let inbox_uid_index = format!("idx_{}_{}_uid", provider.as_ref(), MailBox::Inbox.as_ref());
    let inbox_date_index = format!("idx_{}_{}_date", provider.as_ref(), MailBox::Inbox.as_ref());

    let sent_uid_index = format!("idx_{}_{}_uid", provider.as_ref(), MailBox::Sent.as_ref());
    let sent_date_index = format!("idx_{}_{}_date", provider.as_ref(), MailBox::Sent.as_ref());
    let spam_uid_index = format!("idx_{}_{}_uid", provider.as_ref(), MailBox::Spam.as_ref());
    let spam_date_index = format!("idx_{}_{}_date", provider.as_ref(), MailBox::Spam.as_ref());

    let sql = format!(
        r#"
        CREATE TABLE IF NOT EXISTS {inbox_table_name} (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            uid INTEGER NOT NULL UNIQUE,
            date INTEGER,
            body BLOB,
            labels TEXT,
            read INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS {sent_table_name} (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            uid INTEGER NOT NULL UNIQUE,
            date TEXT,
            body BLOB,
            labels TEXT,
            read INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS {spam_table_name} (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            uid INTEGER NOT NULL UNIQUE,
            date TEXT,
            body BLOB,
            labels TEXT,
            read INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS total_emails (
            provider TEXT PRIMARY KEY,
            Sent INTEGER NOT NULL DEFAULT 0,
            INBOX INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS mailbox_sync_state (
            mailbox TEXT PRIMARY KEY,
            last_uid INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS cache_metadata (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS {inbox_uid_index}
        ON {inbox_table_name}(uid);

        CREATE INDEX IF NOT EXISTS {inbox_date_index}
        ON {inbox_table_name}(date);

        CREATE INDEX IF NOT EXISTS {sent_uid_index}
        ON {sent_table_name}(uid);

        CREATE INDEX IF NOT EXISTS {sent_date_index}
        ON {sent_table_name}(date);

        CREATE INDEX IF NOT EXISTS {spam_uid_index}
        ON {spam_table_name}(uid);

        CREATE INDEX IF NOT EXISTS {spam_date_index}
        ON {spam_table_name}(date);
        "#
    );
    sqlx::query(&sql).execute(pool).await?;

    ensure_read_column(pool, &inbox_table_name).await?;
    ensure_read_column(pool, &sent_table_name).await?;
    ensure_read_column(pool, &spam_table_name).await?;
    migrate_to_selective_attachment_cache(
        pool,
        provider,
        [&inbox_table_name, &sent_table_name, &spam_table_name],
    )
    .await?;

    Ok(())
}

async fn migrate_to_selective_attachment_cache(
    pool: &SqlitePool,
    provider: &Provider,
    table_names: [&str; 3],
) -> Result<(), sqlx::Error> {
    const CACHE_VERSION: &str = "2";
    let key = format!("email_cache_version_{}", provider.as_ref());
    let stored_version: Option<(String,)> =
        sqlx::query_as("SELECT value FROM cache_metadata WHERE key = ?")
            .bind(&key)
            .fetch_optional(pool)
            .await?;

    if stored_version
        .as_ref()
        .is_some_and(|version| version.0 == CACHE_VERSION)
    {
        return Ok(());
    }

    let mut tx = pool.begin().await?;
    for table_name in table_names {
        // Keep the existing rows available while the selective attachment
        // cache is rebuilt in the background. Resetting the high-water mark
        // makes the next initial sync replace each legacy full-message row
        // with the metadata-only representation without blanking the UI.
        sqlx::query("DELETE FROM mailbox_sync_state WHERE mailbox = ?")
            .bind(table_name)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query(
        "INSERT INTO cache_metadata (key, value) VALUES (?, ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(CACHE_VERSION)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(())
}

async fn ensure_read_column(pool: &SqlitePool, table_name: &str) -> Result<(), sqlx::Error> {
    let columns: Vec<(i64, String, String, i64, Option<String>, i64)> =
        sqlx::query_as(&format!("PRAGMA table_info({table_name})"))
            .fetch_all(pool)
            .await?;

    if !columns.iter().any(|column| column.1 == "read") {
        sqlx::query(&format!(
            "ALTER TABLE {table_name} ADD COLUMN read INTEGER NOT NULL DEFAULT 0"
        ))
        .execute(pool)
        .await?;
    }

    Ok(())
}

#[derive(Debug, Clone, Eq, Hash, PartialEq)]
pub enum Provider {
    Yahoo,
    Gmail,
    Outlook,
    CustomImapServer,
}

impl AsRef<str> for Provider {
    fn as_ref(&self) -> &str {
        match self {
            Provider::Yahoo => "yahoo",
            Provider::Gmail => "gmail",
            Provider::Outlook => "outlook",
            Provider::CustomImapServer => "custom",
        }
    }
}

impl From<String> for Provider {
    fn from(s: String) -> Self {
        match s.as_str() {
            "yahoo" => Provider::Yahoo,
            "gmail" => Provider::Gmail,
            "outlook" => Provider::Outlook,
            _ => Provider::CustomImapServer, // Default to Yahoo if unknown
        }
    }
}

#[derive(Debug, Clone, Eq, Hash, PartialEq)]
pub enum MailBox {
    Inbox,
    Sent,
    Drafts,
    Trash,
    Spam,
}

impl AsRef<str> for MailBox {
    fn as_ref(&self) -> &str {
        match self {
            MailBox::Inbox => "INBOX",
            MailBox::Sent => "Sent",
            MailBox::Drafts => "Drafts",
            MailBox::Trash => "Trash",
            MailBox::Spam => "Spam",
        }
    }
}

impl From<String> for MailBox {
    fn from(s: String) -> Self {
        match s.as_str() {
            "INBOX" => MailBox::Inbox,
            "Sent" => MailBox::Sent,
            "Drafts" => MailBox::Drafts,
            "Trash" => MailBox::Trash,
            "Spam" => MailBox::Spam,
            _ => MailBox::Inbox, // Default to Inbox if unknown
        }
    }
}

pub struct EmailCount {
    pub inbox_count: u32,
    pub sent_count: u32,
}
