use crate::{
    db::{
        check_email_db_empty, cleanup, db_pool, ensure_account, ensure_mailbox, get_email_by_uid,
        get_emails, get_last_uid, init_db, populate_storage, set_email_read_states, set_last_uid,
    },
    email_cache::{encode_cached_email_content, EmailAttachment, EmailContent, StandardEmail},
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

async fn test_pool(prefix: &str) -> (std::path::PathBuf, sqlx::SqlitePool) {
    let app_dir = unique_test_dir(prefix);
    let pool = init_db(&app_dir, "emails.db")
        .await
        .expect("db should initialize");
    (app_dir, pool)
}

fn email(uid: u32, date: &str, subject: &str, read: bool) -> StandardEmail {
    StandardEmail {
        uid: Some(uid),
        date: Some(parse_date(date)),
        body: Some(
            format!("Subject: {subject}\r\nFrom: Person <person@example.com>\r\nCc: Team <team@example.com>\r\n\r\nBody for {subject}")
                .into_bytes(),
        ),
        labels: Some(vec!["\\Inbox".to_string(), "Project Phoenix".to_string()]),
        read,
    }
}

#[tokio::test]
async fn attachment_metadata_round_trips_without_payload_bytes() {
    let (app_dir, pool) = test_pool("attachment-metadata").await;
    let content = EmailContent {
        subject: "Quarterly report".to_string(),
        sender_name: "Person".to_string(),
        email_from: "person@example.com".to_string(),
        cc: None,
        preview: "Attached".to_string(),
        html_body: String::new(),
        text_body: "Attached".to_string(),
        attachments: vec![EmailAttachment {
            id: 0,
            part_id: "2.1".to_string(),
            file_name: "report.pdf".to_string(),
            content_type: "application/pdf".to_string(),
            size: 4096,
            data: Vec::new(),
        }],
        parts: Vec::new(),
        raw_message: String::new(),
    };
    populate_storage(
        &pool,
        vec![StandardEmail {
            uid: Some(77),
            date: Some(parse_date("2026-01-01T09:00:00+00:00")),
            body: Some(encode_cached_email_content(&content).unwrap()),
            labels: Some(vec!["\\Inbox".to_string()]),
            read: false,
        }],
        "gmail_INBOX",
    )
    .await
    .unwrap();

    let stored = get_email_by_uid(&pool, "gmail_INBOX", 77)
        .await
        .unwrap()
        .unwrap();
    let attachments = stored.body.unwrap().attachments;
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].part_id, "2.1");
    assert_eq!(attachments[0].file_name, "report.pdf");
    assert_eq!(attachments[0].size, 4096);
    assert!(attachments[0].data.is_empty());

    pool.close().await;
    let _ = std::fs::remove_dir_all(app_dir);
}

#[tokio::test]
async fn migration_replaces_legacy_provider_tables() {
    let app_dir = unique_test_dir("drop-legacy");
    let legacy_pool = db_pool(&app_dir, "emails.db")
        .await
        .expect("legacy database should open");
    sqlx::query("CREATE TABLE gmail_INBOX (uid INTEGER PRIMARY KEY, body BLOB)")
        .execute(&legacy_pool)
        .await
        .expect("legacy table should be created");
    legacy_pool.close().await;

    let pool = init_db(&app_dir, "emails.db")
        .await
        .expect("migration should succeed");
    let legacy_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'gmail_INBOX')",
    )
    .fetch_one(&pool)
    .await
    .expect("legacy lookup should work");
    assert!(!legacy_exists);
    pool.close().await;
    let _ = std::fs::remove_dir_all(app_dir);
}

#[tokio::test]
async fn accounts_and_mailboxes_are_stable_and_unique() {
    let (app_dir, pool) = test_pool("account-mailbox").await;
    let first = ensure_account(&pool, "person@example.com", Some("Person"), "gmail")
        .await
        .expect("account should be created");
    let second = ensure_account(&pool, "person@example.com", None, "gmail")
        .await
        .expect("account should be reused");
    assert_eq!(first, second);

    let inbox = ensure_mailbox(&pool, first, "INBOX", "Inbox", "inbox")
        .await
        .expect("mailbox should be created");
    let same_inbox = ensure_mailbox(&pool, first, "INBOX", "Inbox", "inbox")
        .await
        .expect("mailbox should be reused");
    assert_eq!(inbox, same_inbox);

    pool.close().await;
    let _ = std::fs::remove_dir_all(app_dir);
}

#[tokio::test]
async fn messages_are_normalized_and_page_in_date_order() {
    let (app_dir, pool) = test_pool("message-page").await;
    populate_storage(
        &pool,
        vec![
            email(1, "2026-01-01T09:00:00+00:00", "Oldest", false),
            email(3, "2026-01-03T09:00:00+00:00", "Newest", false),
            email(2, "2026-01-02T09:00:00+00:00", "Middle", false),
        ],
        "gmail_INBOX",
    )
    .await
    .expect("messages should be stored");

    let page = get_emails(&pool, "gmail_INBOX", 0, 2)
        .await
        .expect("page should load");
    assert_eq!(
        page.iter().map(|message| message.uid).collect::<Vec<_>>(),
        vec![Some(3), Some(2)]
    );
    let newest = page[0].body.as_ref().expect("body should be cached");
    assert_eq!(newest.subject, "Newest");
    assert_eq!(newest.email_from, "person@example.com");
    assert_eq!(
        newest.cc.as_ref().expect("cc should exist")[0].1,
        "team@example.com"
    );
    assert!(
        newest.raw_message.is_empty(),
        "raw MIME should not be persisted"
    );
    assert_eq!(
        page[0].labels.as_ref().expect("labels should exist").len(),
        2
    );

    let raw_blob_columns: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('messages') WHERE name IN ('body', 'raw_message')",
    )
    .fetch_one(&pool)
    .await
    .expect("message columns should be inspectable");
    assert_eq!(raw_blob_columns, 0);

    pool.close().await;
    let _ = std::fs::remove_dir_all(app_dir);
}

#[tokio::test]
async fn duplicate_uid_updates_read_state_without_replacing_content() {
    let (app_dir, pool) = test_pool("message-upsert").await;
    populate_storage(
        &pool,
        vec![email(10, "2026-01-01T09:00:00+00:00", "Original", false)],
        "gmail_INBOX",
    )
    .await
    .expect("first insert should work");
    populate_storage(
        &pool,
        vec![email(10, "2026-01-02T09:00:00+00:00", "Replacement", true)],
        "gmail_INBOX",
    )
    .await
    .expect("replay should work");

    let stored = get_email_by_uid(&pool, "gmail_INBOX", 10)
        .await
        .expect("lookup should work")
        .expect("message should exist");
    assert!(stored.read);
    assert_eq!(stored.body.expect("body should exist").subject, "Original");

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&pool)
        .await
        .expect("count should work");
    assert_eq!(count, 1);

    pool.close().await;
    let _ = std::fs::remove_dir_all(app_dir);
}

#[tokio::test]
async fn read_state_and_sync_cursor_use_normalized_rows() {
    let (app_dir, pool) = test_pool("read-and-cursor").await;
    assert!(!check_email_db_empty(&pool, "yahoo_INBOX")
        .await
        .expect("empty lookup should work"));
    populate_storage(
        &pool,
        vec![email(41, "2026-01-01T09:00:00+00:00", "Message", false)],
        "yahoo_INBOX",
    )
    .await
    .expect("message should be inserted");
    assert!(check_email_db_empty(&pool, "yahoo_INBOX")
        .await
        .expect("populated lookup should work"));

    let changed = set_email_read_states(&pool, "yahoo_INBOX", &[(41, true), (999, true)])
        .await
        .expect("read states should update");
    assert_eq!(changed, vec![(41, true)]);

    assert_eq!(get_last_uid(&pool, "yahoo_INBOX").await.unwrap(), None);
    set_last_uid(&pool, "yahoo_INBOX", 41)
        .await
        .expect("cursor should update");
    assert_eq!(get_last_uid(&pool, "yahoo_INBOX").await.unwrap(), Some(41));

    pool.close().await;
    let _ = std::fs::remove_dir_all(app_dir);
}

#[tokio::test]
async fn deleting_a_message_cascades_to_content_parts_and_cache_rows() {
    let (app_dir, pool) = test_pool("message-cascade").await;
    let account_id = ensure_account(&pool, "cascade@example.com", None, "gmail")
        .await
        .unwrap();
    let mailbox_id = ensure_mailbox(&pool, account_id, "INBOX", "Inbox", "inbox")
        .await
        .unwrap();
    let message_id: i64 =
        sqlx::query_scalar("INSERT INTO messages (mailbox_id, uid) VALUES (?, 7) RETURNING id")
            .bind(mailbox_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let part_id: i64 = sqlx::query_scalar(
        "INSERT INTO mime_parts (message_id, section, content_type, disposition) VALUES (?, '2', 'application/pdf', 'attachment') RETURNING id",
    )
    .bind(message_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO part_cache (part_id, relative_path, byte_size, state) VALUES (?, 'aa/file.bin', 10, 'ready')")
        .bind(part_id)
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query("DELETE FROM messages WHERE id = ?")
        .bind(message_id)
        .execute(&pool)
        .await
        .unwrap();
    let parts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mime_parts")
        .fetch_one(&pool)
        .await
        .unwrap();
    let cached: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM part_cache")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!((parts, cached), (0, 0));
    let queued: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM cache_deletion_queue WHERE relative_path = 'aa/file.bin'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(queued, 1);

    pool.close().await;
    let _ = std::fs::remove_dir_all(app_dir);
}

#[tokio::test]
async fn cleanup_handles_file_and_missing_path() {
    let temp_file = unique_test_dir("cleanup-file").with_extension("tmp");
    std::fs::write(&temp_file, b"temporary").expect("temp file should be created");
    cleanup(temp_file.clone())
        .await
        .expect("cleanup should succeed");
    cleanup(temp_file)
        .await
        .expect("missing cleanup should succeed");
}
