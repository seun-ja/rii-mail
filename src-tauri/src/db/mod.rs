mod emails_db;

use std::{fs, path::PathBuf};

pub use emails_db::{check_email_db_empty, cleanup, get_emails, populate_storage};

use sqlx::{sqlite::SqliteConnectOptions, SqlitePool};

use crate::error::Error;

pub async fn init_db(app_dir: PathBuf, db: &str) -> Result<SqlitePool, Error> {
    fs::create_dir_all(&app_dir)?;

    let data_dir = app_dir.join("data");
    fs::create_dir_all(&data_dir)?;

    let db_path = data_dir.join(db);

    let options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true);

    let pool = SqlitePool::connect_with(options).await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS emails (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            date TEXT,
            body BLOB,
            labels TEXT
        );
        "#,
    )
    .execute(&pool)
    .await?;

    Ok(pool)
}
