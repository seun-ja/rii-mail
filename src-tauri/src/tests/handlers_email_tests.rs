use crate::{
    db::{init_db, populate_storage, MailBox, Provider},
    email_cache::StandardEmail,
    handlers::email::get_emails_as_front_end_from_pool,
};
use chrono::{DateTime, FixedOffset};
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_test_dir(prefix: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be monotonic for tests")
        .as_nanos();
    std::env::temp_dir().join(format!("riimail-{prefix}-{nanos}"))
}

fn parse_date(value: &str) -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339(value).expect("date should parse")
}

#[tokio::test]
async fn get_emails_as_front_end_from_pool_maps_rows_to_frontend_shape() {
    let app_dir = unique_test_dir("handler-email");
    let pool = init_db(&app_dir, "emails.db", &Provider::Gmail)
        .await
        .expect("db should initialize");

    populate_storage(
        &pool,
        vec![StandardEmail {
            uid: Some(100),
            date: Some(parse_date("2026-01-03T09:00:00+00:00")),
            body: Some(
                b"Subject: Welcome\r\nFrom: alice@example.com\r\n\r\nHello from test".to_vec(),
            ),
            labels: Some(vec!["\\Inbox".to_string()]),
        }],
        "gmail_INBOX",
    )
    .await
    .expect("storage should be populated");

    let rows = get_emails_as_front_end_from_pool(Provider::Gmail, MailBox::Inbox, 0, 10, &pool)
        .await
        .expect("handler conversion should succeed");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "db-INBOX-0");
    assert_eq!(rows[0].folder, "INBOX");
    assert_eq!(rows[0].subject, "Welcome");

    pool.close().await;
    let _ = std::fs::remove_dir_all(app_dir);
}
