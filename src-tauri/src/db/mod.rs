mod emails_db;

use std::{fs, path::PathBuf};

pub use emails_db::populate_storage;

use sqlx::{sqlite::SqliteConnectOptions, SqlitePool};

pub async fn init_db(
    app_dir: PathBuf,
    db: &str,
) -> Result<SqlitePool, Box<dyn std::error::Error + Send + Sync>> {
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
