use sqlx::{QueryBuilder, SqlitePool};

use crate::{email_cache::Email, error::Error};

pub async fn populate_storage(pool: SqlitePool, emails: Vec<Email>) -> Result<(), Error> {
    if emails.is_empty() {
        return Ok(());
    }

    let mut query_builder: QueryBuilder<'_, sqlx::Sqlite> =
        QueryBuilder::new("INSERT INTO emails (date, body, labels) ");

    let emails_data: Vec<_> = emails
        .into_iter()
        .map(|email| {
            let labels_str = email.labels.map(|labels| labels.join(","));
            (email.date, email.body, labels_str)
        })
        .collect();

    query_builder.push_values(emails_data, |mut b, (date, body, labels_str)| {
        b.push_bind(date);
        b.push_bind(body);
        b.push_bind(labels_str);
    });

    let query = query_builder.build();
    query.execute(&pool).await?;

    Ok(())
}
