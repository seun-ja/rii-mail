mod emails_db;

use std::{fs, path::PathBuf};

pub use emails_db::{
    check_email_db_empty, cleanup, get_emails, get_last_uid, populate_inbox_folder_count,
    populate_sent_folder_count, populate_storage, set_last_uid,
};

use sqlx::{sqlite::SqliteConnectOptions, SqlitePool};

use crate::error::Error;

pub async fn db_pool(app_dir: &PathBuf, db: &str) -> Result<SqlitePool, sqlx::Error> {
    fs::create_dir_all(app_dir)?;

    let data_dir = app_dir.join("data");
    fs::create_dir_all(&data_dir)?;

    let db_path = data_dir.join(db);

    let options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true);

    SqlitePool::connect_with(options).await
}

pub async fn init_db(
    app_dir: &PathBuf,
    db: &str,
    provider: &Provider,
) -> Result<SqlitePool, Error> {
    let pool = db_pool(app_dir, db).await?;

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

    let inbox_uid_index = format!("idx_{}_{}_uid", provider.as_ref(), MailBox::Inbox.as_ref());
    let inbox_date_index = format!("idx_{}_{}_date", provider.as_ref(), MailBox::Inbox.as_ref());

    let sent_uid_index = format!("idx_{}_{}_uid", provider.as_ref(), MailBox::Sent.as_ref());
    let sent_date_index = format!("idx_{}_{}_date", provider.as_ref(), MailBox::Sent.as_ref());

    let sql = format!(
        r#"
        CREATE TABLE IF NOT EXISTS {inbox_table_name} (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            uid INTEGER NOT NULL UNIQUE,
            date INTEGER,
            body BLOB,
            labels TEXT
        );

        CREATE TABLE IF NOT EXISTS {sent_table_name} (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            uid INTEGER NOT NULL UNIQUE,
            date TEXT,
            body BLOB,
            labels TEXT
        );

        CREATE TABLE total_emails (
            provider TEXT PRIMARY KEY,
            Sent INTEGER NOT NULL DEFAULT 0,
            INBOX INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS mailbox_sync_state (
            mailbox TEXT PRIMARY KEY,
            last_uid INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS {inbox_uid_index}
        ON {inbox_table_name}(uid);

        CREATE INDEX IF NOT EXISTS {inbox_date_index}
        ON {inbox_table_name}(date);

        CREATE INDEX IF NOT EXISTS {sent_uid_index}
        ON {sent_table_name}(uid);

        CREATE INDEX IF NOT EXISTS {sent_date_index}
        ON {sent_table_name}(date);
        "#
    );
    sqlx::query(&sql).execute(pool).await?;

    Ok(())
}

#[derive(Debug, Clone, Eq, Hash, PartialEq)]
pub enum Provider {
    Yahoo,
    Gmail,
}

impl AsRef<str> for Provider {
    fn as_ref(&self) -> &str {
        match self {
            Provider::Yahoo => "yahoo",
            Provider::Gmail => "gmail",
        }
    }
}

impl From<String> for Provider {
    fn from(s: String) -> Self {
        match s.as_str() {
            "yahoo" => Provider::Yahoo,
            "gmail" => Provider::Gmail,
            _ => Provider::Yahoo, // Default to Yahoo if unknown
        }
    }
}

#[derive(Debug, Clone, Eq, Hash, PartialEq)]
pub enum MailBox {
    Inbox,
    Sent,
    Drafts,
    Trash,
}

impl AsRef<str> for MailBox {
    fn as_ref(&self) -> &str {
        match self {
            MailBox::Inbox => "INBOX",
            MailBox::Sent => "Sent",
            MailBox::Drafts => "Drafts",
            MailBox::Trash => "Trash",
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
            _ => MailBox::Inbox, // Default to Inbox if unknown
        }
    }
}

pub struct EmailCount {
    pub inbox_count: u32,
    pub sent_count: u32,
}
