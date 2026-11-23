mod emails_db;

use std::{fs, path::PathBuf, time::Duration};

pub use emails_db::{
    check_email_db_empty, cleanup, ensure_account, ensure_mailbox, get_email_by_uid, get_emails,
    get_last_uid, populate_inbox_folder_count, populate_sent_folder_count, populate_storage,
    set_email_read, set_email_read_states, set_last_uid,
};

use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous},
    SqlitePool,
};

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
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5));

    SqlitePool::connect_with(options).await
}

#[tracing::instrument(
    name = "db.initialize",
    skip_all,
    fields(database = db),
    err
)]
pub async fn init_db(app_data_dir: &PathBuf, db: &str) -> Result<SqlitePool, Error> {
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

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|error| Error::Other(format!("Database migration failed: {error}")))?;

    Ok(pool)
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
