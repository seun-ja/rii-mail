use crate::db::{init_db, MailBox, Provider};
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_test_dir(prefix: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be monotonic for tests")
        .as_nanos();
    std::env::temp_dir().join(format!("riimail-{prefix}-{nanos}"))
}

#[test]
fn providers_as_ref_and_from_string() {
    assert_eq!(Provider::Yahoo.as_ref(), "yahoo");
    assert_eq!(Provider::Gmail.as_ref(), "gmail");
    assert!(matches!(
        Provider::from("outlook".to_string()),
        Provider::Outlook
    ));
    assert!(matches!(
        Provider::from("unknown".to_string()),
        Provider::CustomImapServer
    ));
}

#[test]
fn mailbox_as_ref_and_from_string() {
    assert_eq!(MailBox::Inbox.as_ref(), "INBOX");
    assert_eq!(MailBox::Sent.as_ref(), "Sent");
    assert_eq!(MailBox::Drafts.as_ref(), "Drafts");
    assert_eq!(MailBox::Trash.as_ref(), "Trash");
    assert_eq!(MailBox::Spam.as_ref(), "Spam");
    assert!(matches!(MailBox::from("Spam".to_string()), MailBox::Spam));
}

#[tokio::test]
async fn init_db_runs_the_standard_schema_migration() {
    let app_dir = unique_test_dir("standard-schema");
    let pool = init_db(&app_dir, "emails.db")
        .await
        .expect("db should initialize");

    let expected = [
        "accounts",
        "mailboxes",
        "messages",
        "message_addresses",
        "labels",
        "message_labels",
        "message_content",
        "mime_parts",
        "part_cache",
        "cache_deletion_queue",
        "drafts",
        "draft_addresses",
        "draft_attachments",
    ];
    for table in expected {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?)",
        )
        .bind(table)
        .fetch_one(&pool)
        .await
        .expect("schema lookup should work");
        assert!(exists, "{table} should exist");
    }

    let provider_tables: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('yahoo_INBOX', 'gmail_INBOX', 'outlook_INBOX')",
    )
    .fetch_one(&pool)
    .await
    .expect("legacy table lookup should work");
    assert_eq!(provider_tables, 0);

    let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&pool)
        .await
        .expect("foreign key pragma should be readable");
    assert_eq!(foreign_keys, 1);

    pool.close().await;
    let _ = std::fs::remove_dir_all(app_dir);
}
