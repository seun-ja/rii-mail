mod emails_db;

pub use emails_db::populate_storage;

use sqlx::SqlitePool;

pub async fn init_db(
    location: &str,
) -> Result<SqlitePool, Box<dyn std::error::Error + Send + Sync>> {
    let pool = SqlitePool::connect(location).await?;

    Ok(pool)
}
