use crate::{
    db::{
        check_email_db_empty, cleanup, get_emails, get_last_uid, init_db, populate_storage,
        set_last_uid, Provider,
    },
    email_cache::Email,
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
async fn populate_storage_and_get_emails_work_with_pagination() {
    let app_dir = unique_test_dir("emails-pagination");
    let pool = init_db(&app_dir, "emails.db", &Provider::Gmail)
        .await
        .expect("db should initialize");

    let table = "gmail_INBOX";

    let emails = vec![
        Email {
            uid: Some(1),
            date: Some(parse_date("2026-01-01T09:00:00+00:00")),
            body: Some(b"Subject: Older\r\nFrom: old@example.com\r\n\r\nBody".to_vec()),
            labels: Some(vec!["\\Inbox".to_string()]),
        },
        Email {
            uid: Some(3),
            date: Some(parse_date("2026-01-03T09:00:00+00:00")),
            body: Some(b"Subject: Newest\r\nFrom: newest@example.com\r\n\r\nBody".to_vec()),
            labels: Some(vec!["\\Inbox".to_string()]),
        },
        Email {
            uid: Some(2),
            date: Some(parse_date("2026-01-02T09:00:00+00:00")),
            body: Some(b"Subject: Middle\r\nFrom: middle@example.com\r\n\r\nBody".to_vec()),
            labels: Some(vec!["\\Inbox".to_string()]),
        },
    ];

    populate_storage(&pool, emails, table)
        .await
        .expect("populate should succeed");

    let first_page = get_emails(&pool, table, 0, 2)
        .await
        .expect("query should succeed");
    assert_eq!(first_page.len(), 2);
    assert!(first_page[0]
        .body
        .as_ref()
        .expect("email body should exist")
        .subject
        .contains("Newest"));

    let second_page = get_emails(&pool, table, 2, 3)
        .await
        .expect("query should succeed");
    assert_eq!(second_page.len(), 1);
    assert!(second_page[0]
        .body
        .as_ref()
        .expect("email body should exist")
        .subject
        .contains("Older"));

    pool.close().await;
    let _ = std::fs::remove_dir_all(app_dir);
}

#[tokio::test]
async fn populate_storage_with_empty_input_is_noop() {
    let app_dir = unique_test_dir("emails-empty");
    let pool = init_db(&app_dir, "emails.db", &Provider::Yahoo)
        .await
        .expect("db should initialize");

    populate_storage(&pool, Vec::new(), "yahoo_INBOX")
        .await
        .expect("empty insert should succeed");

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM yahoo_INBOX")
        .fetch_one(&pool)
        .await
        .expect("count query should succeed");

    assert_eq!(count.0, 0);

    pool.close().await;
    let _ = std::fs::remove_dir_all(app_dir);
}

#[tokio::test]
async fn populate_storage_ignores_duplicate_uids() {
    let app_dir = unique_test_dir("emails-dup-uids");
    let pool = init_db(&app_dir, "emails.db", &Provider::Gmail)
        .await
        .expect("db should initialize");

    let table = "gmail_INBOX";

    let first_insert = vec![Email {
        uid: Some(10),
        date: Some(parse_date("2026-01-10T09:00:00+00:00")),
        body: Some(b"Subject: First\r\nFrom: first@example.com\r\n\r\nBody".to_vec()),
        labels: Some(vec!["\\Inbox".to_string()]),
    }];

    let duplicate_insert = vec![Email {
        uid: Some(10),
        date: Some(parse_date("2026-01-11T09:00:00+00:00")),
        body: Some(b"Subject: Duplicate\r\nFrom: dup@example.com\r\n\r\nBody".to_vec()),
        labels: Some(vec!["\\Inbox".to_string()]),
    }];

    populate_storage(&pool, first_insert, table)
        .await
        .expect("first insert should succeed");
    populate_storage(&pool, duplicate_insert, table)
        .await
        .expect("duplicate insert should be ignored and succeed");

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM gmail_INBOX")
        .fetch_one(&pool)
        .await
        .expect("count query should succeed");

    assert_eq!(count.0, 1);

    pool.close().await;
    let _ = std::fs::remove_dir_all(app_dir);
}

#[tokio::test]
async fn populate_storage_keeps_all_unique_uids_when_replayed() {
    let app_dir = unique_test_dir("emails-replay");
    let pool = init_db(&app_dir, "emails.db", &Provider::Yahoo)
        .await
        .expect("db should initialize");

    let table = "yahoo_INBOX";

    let initial = vec![
        Email {
            uid: Some(1),
            date: Some(parse_date("2026-02-01T09:00:00+00:00")),
            body: Some(b"Subject: One\r\nFrom: one@example.com\r\n\r\nBody".to_vec()),
            labels: None,
        },
        Email {
            uid: Some(2),
            date: Some(parse_date("2026-02-02T09:00:00+00:00")),
            body: Some(b"Subject: Two\r\nFrom: two@example.com\r\n\r\nBody".to_vec()),
            labels: None,
        },
    ];

    let replay_plus_new = vec![
        Email {
            uid: Some(1),
            date: Some(parse_date("2026-02-01T09:00:00+00:00")),
            body: Some(b"Subject: One\r\nFrom: one@example.com\r\n\r\nBody".to_vec()),
            labels: None,
        },
        Email {
            uid: Some(2),
            date: Some(parse_date("2026-02-02T09:00:00+00:00")),
            body: Some(b"Subject: Two\r\nFrom: two@example.com\r\n\r\nBody".to_vec()),
            labels: None,
        },
        Email {
            uid: Some(3),
            date: Some(parse_date("2026-02-03T09:00:00+00:00")),
            body: Some(b"Subject: Three\r\nFrom: three@example.com\r\n\r\nBody".to_vec()),
            labels: None,
        },
    ];

    populate_storage(&pool, initial, table)
        .await
        .expect("initial insert should succeed");
    populate_storage(&pool, replay_plus_new, table)
        .await
        .expect("replayed insert should succeed");

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM yahoo_INBOX")
        .fetch_one(&pool)
        .await
        .expect("count query should succeed");

    assert_eq!(count.0, 3);

    pool.close().await;
    let _ = std::fs::remove_dir_all(app_dir);
}

#[tokio::test]
async fn check_email_db_empty_reflects_table_state() {
    let app_dir = unique_test_dir("emails-empty-flag");
    let pool = init_db(&app_dir, "emails.db", &Provider::Yahoo)
        .await
        .expect("db should initialize");

    assert!(!check_email_db_empty(&pool, "yahoo_INBOX")
        .await
        .expect("exists query should succeed"));

    populate_storage(
        &pool,
        vec![Email {
            uid: Some(1),
            date: Some(parse_date("2026-01-01T09:00:00+00:00")),
            body: Some(b"Subject: One\r\nFrom: one@example.com\r\n\r\nBody".to_vec()),
            labels: None,
        }],
        "yahoo_INBOX",
    )
    .await
    .expect("insert should succeed");

    assert!(check_email_db_empty(&pool, "yahoo_INBOX")
        .await
        .expect("exists query should succeed"));

    pool.close().await;
    let _ = std::fs::remove_dir_all(app_dir);
}

#[tokio::test]
async fn mailbox_sync_state_roundtrip_works() {
    let app_dir = unique_test_dir("emails-last-uid");
    let pool = init_db(&app_dir, "emails.db", &Provider::Gmail)
        .await
        .expect("db should initialize");

    assert_eq!(
        get_last_uid(&pool, "gmail_INBOX")
            .await
            .expect("query should succeed"),
        None
    );

    set_last_uid(&pool, "gmail_INBOX", 41)
        .await
        .expect("set should succeed");
    assert_eq!(
        get_last_uid(&pool, "gmail_INBOX")
            .await
            .expect("query should succeed"),
        Some(41)
    );

    set_last_uid(&pool, "gmail_INBOX", 42)
        .await
        .expect("update should succeed");
    assert_eq!(
        get_last_uid(&pool, "gmail_INBOX")
            .await
            .expect("query should succeed"),
        Some(42)
    );

    pool.close().await;
    let _ = std::fs::remove_dir_all(app_dir);
}

#[tokio::test]
async fn cleanup_handles_file_and_missing_path() {
    let temp_file = unique_test_dir("cleanup-file").with_extension("tmp");
    std::fs::write(&temp_file, b"temporary").expect("temp file should be created");

    cleanup(temp_file.clone())
        .await
        .expect("cleanup file should succeed");
    assert!(!temp_file.exists());

    cleanup(temp_file)
        .await
        .expect("cleanup missing path should be ok");
}
