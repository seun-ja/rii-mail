use crate::db::{init_db, MailBox, Providers};
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
    assert_eq!(Providers::Yahoo.as_ref(), "yahoo");
    assert_eq!(Providers::Gmail.as_ref(), "gmail");

    assert!(matches!(
        Providers::from("yahoo".to_string()),
        Providers::Yahoo
    ));
    assert!(matches!(
        Providers::from("gmail".to_string()),
        Providers::Gmail
    ));
    assert!(matches!(
        Providers::from("unknown".to_string()),
        Providers::Yahoo
    ));
}

#[test]
fn mailbox_as_ref_and_from_string() {
    assert_eq!(MailBox::Inbox.as_ref(), "INBOX");
    assert_eq!(MailBox::Sent.as_ref(), "Sent");
    assert_eq!(MailBox::Drafts.as_ref(), "Drafts");
    assert_eq!(MailBox::Trash.as_ref(), "Trash");

    assert!(matches!(MailBox::from("INBOX".to_string()), MailBox::Inbox));
    assert!(matches!(MailBox::from("Sent".to_string()), MailBox::Sent));
    assert!(matches!(
        MailBox::from("Drafts".to_string()),
        MailBox::Drafts
    ));
    assert!(matches!(MailBox::from("Trash".to_string()), MailBox::Trash));
    assert!(matches!(MailBox::from("other".to_string()), MailBox::Inbox));
}

#[tokio::test]
async fn init_db_creates_provider_tables_and_state_table() {
    let app_dir = unique_test_dir("init-db");

    let pool = init_db(&app_dir, "emails.db", &Providers::Yahoo)
        .await
        .expect("db should initialize");

    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('yahoo_INBOX', 'yahoo_Sent', 'mailbox_sync_state')",
    )
    .fetch_one(&pool)
    .await
    .expect("table listing should work");

    assert_eq!(count.0, 3);

    pool.close().await;

    let _ = std::fs::remove_dir_all(app_dir);
}
