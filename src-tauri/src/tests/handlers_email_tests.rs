use crate::{
    db::{init_db, populate_storage, MailBox, Provider},
    email_cache::StandardEmail,
    handlers::email::{
        attachment_cache_path, get_emails_as_front_end_from_pool, save_attachment_to_directory,
        EmailAttachmentData,
    },
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

#[test]
fn attachment_cache_paths_are_isolated_by_account_and_uid_validity() {
    let root = std::path::Path::new("/cache");
    let first = attachment_cache_path(
        root,
        "first@example.com",
        &Provider::Gmail,
        &MailBox::Inbox,
        10,
        42,
        "2.1",
    )
    .expect("cache path should build");
    let other_account = attachment_cache_path(
        root,
        "second@example.com",
        &Provider::Gmail,
        &MailBox::Inbox,
        10,
        42,
        "2.1",
    )
    .expect("cache path should build");
    let other_mailbox_generation = attachment_cache_path(
        root,
        "first@example.com",
        &Provider::Gmail,
        &MailBox::Inbox,
        11,
        42,
        "2.1",
    )
    .expect("cache path should build");

    assert_ne!(first, other_account);
    assert_ne!(first, other_mailbox_generation);
    assert!(!first.to_string_lossy().contains("first@example.com"));
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
            read: false,
        }],
        "gmail_INBOX",
    )
    .await
    .expect("storage should be populated");

    let rows = get_emails_as_front_end_from_pool(Provider::Gmail, MailBox::Inbox, 0, 10, &pool)
        .await
        .expect("handler conversion should succeed");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "db-INBOX-uid-100");
    assert_eq!(rows[0].uid, Some(100));
    assert_eq!(rows[0].folder, "INBOX");
    assert_eq!(rows[0].subject, "Welcome");

    pool.close().await;
    let _ = std::fs::remove_dir_all(app_dir);
}

#[tokio::test]
async fn save_attachment_to_directory_writes_bytes_without_overwriting_existing_files() {
    let download_dir = unique_test_dir("attachment-downloads");
    let attachment = EmailAttachmentData {
        file_name: "report.pdf".to_string(),
        data: vec![1, 2, 3, 4],
    };

    let first_path = save_attachment_to_directory(&download_dir, &attachment)
        .await
        .expect("first attachment should save");
    let second_path = save_attachment_to_directory(&download_dir, &attachment)
        .await
        .expect("duplicate attachment should save with a unique name");

    assert_eq!(
        first_path.file_name().and_then(|name| name.to_str()),
        Some("report.pdf")
    );
    assert_eq!(
        second_path.file_name().and_then(|name| name.to_str()),
        Some("report (1).pdf")
    );
    assert_eq!(
        tokio::fs::read(first_path)
            .await
            .expect("saved attachment should be readable"),
        attachment.data
    );

    let unsafe_name = EmailAttachmentData {
        file_name: "..".to_string(),
        data: vec![5],
    };
    let safe_path = save_attachment_to_directory(&download_dir, &unsafe_name)
        .await
        .expect("unsafe attachment names should fall back safely");
    assert_eq!(
        safe_path.file_name().and_then(|name| name.to_str()),
        Some("attachment")
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn database_email_ids_remain_stable_when_a_newer_email_is_inserted() {
    let app_dir = unique_test_dir("handler-email-stable-ids");
    let pool = init_db(&app_dir, "emails.db", &Provider::Gmail)
        .await
        .expect("db should initialize");

    populate_storage(
        &pool,
        vec![StandardEmail {
            uid: Some(100),
            date: Some(parse_date("2026-01-03T09:00:00+00:00")),
            body: Some(b"Subject: Existing\r\nFrom: old@example.com\r\n\r\nOld".to_vec()),
            labels: Some(vec!["\\Inbox".to_string()]),
            read: false,
        }],
        "gmail_INBOX",
    )
    .await
    .expect("initial email should be stored");

    let before = get_emails_as_front_end_from_pool(Provider::Gmail, MailBox::Inbox, 0, 10, &pool)
        .await
        .expect("initial page should load");

    populate_storage(
        &pool,
        vec![StandardEmail {
            uid: Some(101),
            date: Some(parse_date("2026-01-04T09:00:00+00:00")),
            body: Some(b"Subject: New\r\nFrom: new@example.com\r\n\r\nNew".to_vec()),
            labels: Some(vec!["\\Inbox".to_string()]),
            read: false,
        }],
        "gmail_INBOX",
    )
    .await
    .expect("new email should be stored");

    let after = get_emails_as_front_end_from_pool(Provider::Gmail, MailBox::Inbox, 0, 10, &pool)
        .await
        .expect("refreshed page should load");

    assert_eq!(before[0].id, "db-INBOX-uid-100");
    assert_eq!(after[0].id, "db-INBOX-uid-101");
    assert_eq!(after[0].subject, "New");
    assert_eq!(after[1].id, before[0].id);

    pool.close().await;
    let _ = std::fs::remove_dir_all(app_dir);
}
