mod emails_db;

use std::{fs, path::PathBuf};

pub use emails_db::{check_email_db_empty, cleanup, get_emails, populate_storage};

use sqlx::{sqlite::SqliteConnectOptions, SqlitePool};

use crate::error::Error;

pub async fn init_db(
    app_dir: PathBuf,
    db: &str,
    _has_logged_in: bool,
    provider: &Providers,
) -> Result<SqlitePool, Error> {
    fs::create_dir_all(&app_dir)?;

    let data_dir = app_dir.join("data");
    fs::create_dir_all(&data_dir)?;

    let db_path = data_dir.join(db);

    let options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true);

    let pool = SqlitePool::connect_with(options).await?;

    create_table_for_provider(&pool, provider).await?;

    Ok(pool)
}

async fn create_table_for_provider(
    pool: &SqlitePool,
    provider: &Providers,
) -> Result<(), sqlx::Error> {
    let inbox_table_name = format!("{}_{}", provider.as_ref(), MailBox::Inbox.as_ref());
    let sent_table_name = format!("{}_{}", provider.as_ref(), MailBox::Sent.as_ref());

    let create_inbox_sql = format!(
        "CREATE TABLE IF NOT EXISTS {inbox_table_name} (\
            id INTEGER PRIMARY KEY AUTOINCREMENT,\
            date TEXT,\
            body BLOB,\
            labels TEXT\
        );"
    );
    sqlx::query(&create_inbox_sql).execute(pool).await?;

    let create_sent_sql = format!(
        "CREATE TABLE IF NOT EXISTS {sent_table_name} (\
            id INTEGER PRIMARY KEY AUTOINCREMENT,\
            date TEXT,\
            body BLOB,\
            labels TEXT\
        );"
    );
    sqlx::query(&create_sent_sql).execute(pool).await?;

    Ok(())
}

#[derive(Debug, Clone)]
pub enum Providers {
    Yahoo,
    Gmail,
}

impl AsRef<str> for Providers {
    fn as_ref(&self) -> &str {
        match self {
            Providers::Yahoo => "yahoo",
            Providers::Gmail => "gmail",
        }
    }
}

impl From<String> for Providers {
    fn from(s: String) -> Self {
        match s.as_str() {
            "yahoo" => Providers::Yahoo,
            "gmail" => Providers::Gmail,
            _ => Providers::Yahoo, // Default to Yahoo if unknown
        }
    }
}

#[derive(Debug, Clone)]
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
