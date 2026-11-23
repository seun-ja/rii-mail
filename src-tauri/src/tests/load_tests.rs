use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{TimeZone as _, Utc};

use crate::{
    db::{get_emails, init_db, populate_storage, set_last_uid},
    email_cache::{encode_cached_email_content, EmailAttachment, EmailContent, StandardEmail},
};

const EMAIL_COUNT: u32 = 100_000;
const ATTACHMENT_COUNT: u32 = 1_000;
const INSERT_BATCH_SIZE: u32 = 1_000;
const FIRST_PAGE_SIZE: u32 = 100;

fn unique_test_dir() -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should move forward")
        .as_nanos();
    std::env::temp_dir().join(format!("riimail-load-100k-{nanos}"))
}

fn duration_budget(name: &str, default: Duration) -> Duration {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(default)
}

fn fixture(uid: u32) -> StandardEmail {
    let has_attachment = uid <= ATTACHMENT_COUNT;
    let attachments = has_attachment
        .then(|| EmailAttachment {
            id: 0,
            part_id: "2".to_string(),
            file_name: format!("document-{uid}.pdf"),
            content_type: "application/pdf".to_string(),
            size: 128 * 1024,
            data: Vec::new(),
        })
        .into_iter()
        .collect();
    let content = EmailContent {
        subject: format!("Load test message {uid}"),
        sender_name: format!("Sender {uid}"),
        email_from: format!("sender-{uid}@example.com"),
        cc: None,
        preview: format!("Preview for load test message {uid}"),
        html_body: String::new(),
        text_body: format!("Body for load test message {uid}"),
        attachments,
        parts: Vec::new(),
        raw_message: String::new(),
    };

    StandardEmail {
        uid: Some(uid),
        date: Utc
            .timestamp_opt(1_700_000_000 + i64::from(uid), 0)
            .single()
            .map(|date| date.fixed_offset()),
        body: Some(encode_cached_email_content(&content).expect("fixture should serialize")),
        labels: None,
        read: uid % 3 == 0,
    }
}

/// Opt-in normalized database load test for a large returning-user mailbox.
///
/// Run with: `npm run test:load:100k`
#[tokio::test(flavor = "multi_thread")]
#[ignore = "100k-email load test; run explicitly"]
async fn load_100k_mailbox() {
    let app_dir = unique_test_dir();
    let database_path = app_dir.join("data/emails.db");
    let pool = init_db(&app_dir, "emails.db")
        .await
        .expect("load-test database should initialize");

    let ingestion_started = Instant::now();
    for first_uid in (1..=EMAIL_COUNT).step_by(INSERT_BATCH_SIZE as usize) {
        let last_uid = (first_uid + INSERT_BATCH_SIZE - 1).min(EMAIL_COUNT);
        let batch = (first_uid..=last_uid).map(fixture).collect::<Vec<_>>();
        populate_storage(&pool, batch, "gmail_INBOX")
            .await
            .expect("load-test batch should be stored");
    }
    set_last_uid(&pool, "gmail_INBOX", EMAIL_COUNT)
        .await
        .expect("sync cursor should be stored");
    let ingestion_elapsed = ingestion_started.elapsed();

    let first_page_started = Instant::now();
    let first_page = get_emails(&pool, "gmail_INBOX", 0, FIRST_PAGE_SIZE)
        .await
        .expect("first page should load");
    let first_page_elapsed = first_page_started.elapsed();

    let deep_page_started = Instant::now();
    let deep_page = get_emails(
        &pool,
        "gmail_INBOX",
        EMAIL_COUNT - FIRST_PAGE_SIZE,
        EMAIL_COUNT,
    )
    .await
    .expect("deep production page should load");
    let deep_page_elapsed = deep_page_started.elapsed();

    let message_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&pool)
        .await
        .expect("message count should load");
    let attachment_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mime_parts")
        .fetch_one(&pool)
        .await
        .expect("attachment count should load");
    let cached_payload_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM part_cache")
        .fetch_one(&pool)
        .await
        .expect("part cache count should load");

    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(&pool)
        .await
        .expect("WAL checkpoint should complete");
    pool.close().await;
    let database_bytes = std::fs::metadata(&database_path)
        .expect("database file should exist")
        .len();
    let _ = std::fs::remove_dir_all(&app_dir);

    println!(
        "100k load test: ingest={ingestion_elapsed:?}, first_page={first_page_elapsed:?}, deep_page={deep_page_elapsed:?}, db={:.1} MiB, attachments={attachment_count}",
        database_bytes as f64 / (1024.0 * 1024.0),
    );

    assert_eq!(message_count, i64::from(EMAIL_COUNT));
    assert_eq!(attachment_count, i64::from(ATTACHMENT_COUNT));
    assert_eq!(cached_payload_count, 0, "sync must not cache payload bytes");
    assert_eq!(first_page.len(), FIRST_PAGE_SIZE as usize);
    assert_eq!(deep_page.len(), FIRST_PAGE_SIZE as usize);
    assert!(
        ingestion_elapsed
            <= duration_budget("RIIMAIL_LOAD_INGEST_BUDGET_MS", Duration::from_secs(180)),
        "100k ingestion exceeded its budget: {ingestion_elapsed:?}"
    );
    assert!(
        first_page_elapsed
            <= duration_budget("RIIMAIL_LOAD_FIRST_PAGE_BUDGET_MS", Duration::from_secs(1)),
        "first page exceeded its budget: {first_page_elapsed:?}"
    );
    assert!(
        deep_page_elapsed
            <= duration_budget("RIIMAIL_LOAD_DEEP_PAGE_BUDGET_MS", Duration::from_secs(3)),
        "deep page exceeded its budget: {deep_page_elapsed:?}"
    );
    assert!(
        database_bytes <= 512 * 1024 * 1024,
        "metadata-only database unexpectedly exceeded 512 MiB"
    );
}
